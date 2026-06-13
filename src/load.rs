use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use minijinja::context;

use crate::dirs::{BaseDirs, expand_tilde};
use crate::model::{DefFile, Entry, PathType, RawDef};
use crate::template;

/// A definition together with the file it came from.
pub struct LoadedDef {
    pub file: PathBuf,
    pub def: RawDef,
}

pub struct Definitions {
    pub defs: Vec<LoadedDef>,
    pub vars: toml::Table,
}

/// Definition files in load order: `paths.toml`, then `paths.d/*.toml`
/// sorted by file name.
pub fn definition_files(config_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let main = config_dir.join("paths.toml");
    if main.is_file() {
        files.push(main);
    }
    if let Ok(entries) = fs::read_dir(config_dir.join("paths.d")) {
        let mut extra: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml") && p.is_file())
            .collect();
        extra.sort();
        files.append(&mut extra);
    }
    files
}

pub fn load(config_dir: &Path) -> Result<Definitions> {
    let mut defs = Vec::new();
    let mut vars = toml::Table::new();
    for file in definition_files(config_dir) {
        let text =
            fs::read_to_string(&file).with_context(|| format!("cannot read {}", file.display()))?;
        let parsed: DefFile =
            toml::from_str(&text).with_context(|| format!("cannot parse {}", file.display()))?;
        for (k, v) in parsed.vars {
            vars.insert(k, v);
        }
        defs.extend(parsed.paths.into_iter().map(|def| LoadedDef {
            file: file.clone(),
            def,
        }));
    }
    Ok(Definitions { defs, vars })
}

/// Renders path templates and applies the type/existence filter, on demand.
///
/// Templates may run shell commands or globs, so definitions are evaluated
/// only when needed.  When an abbreviation is defined more than once, the last
/// surviving definition in load order wins (like `git config`), so callers
/// evaluate candidates from the end and stop at the first that survives.
pub struct Resolver<'a> {
    env: minijinja::Environment<'static>,
    home: std::path::PathBuf,
    defs: &'a [LoadedDef],
}

impl<'a> Resolver<'a> {
    pub fn new(dirs: &BaseDirs, definitions: &'a Definitions) -> Self {
        Resolver {
            env: template::build_env(dirs, &definitions.vars),
            home: dirs.home.clone(),
            defs: &definitions.defs,
        }
    }

    /// Evaluate one definition, returning the entry if it renders and its
    /// rendered path matches its type.  Render and type errors are reported to
    /// stderr; filtered-out and erroring definitions both yield `None`.
    fn eval(&self, loaded: &LoadedDef) -> Option<Entry> {
        let def = &loaded.def;
        let warn = |msg: &dyn std::fmt::Display| {
            eprintln!(
                "qpath: warning: {}: {}: {msg}",
                loaded.file.display(),
                def.abbr
            );
        };
        let rendered = match self.env.render_str(&def.path, context! {}) {
            Ok(s) => s,
            Err(e) => {
                warn(&e);
                return None;
            }
        };
        let type_ = match &def.type_ {
            Some(s) => match PathType::parse(s) {
                Ok(t) => t,
                Err(e) => {
                    warn(&e);
                    return None;
                }
            },
            None => PathType::infer(&rendered),
        };
        let expanded = expand_tilde(&rendered, &self.home);
        if !type_.matches(Path::new(&expanded)) {
            return None;
        }
        Some(Entry {
            abbr: def.abbr.clone(),
            desc: def.desc.clone(),
            expanded,
            type_,
        })
    }

    /// Resolve the winning entry for one abbreviation under the `type_`
    /// filter, or `None` if no matching definition survives.  Candidates are
    /// evaluated from the last in load order and the first that both exists
    /// and matches `type_` wins; earlier candidates are not evaluated.
    pub fn resolve_abbr(&self, abbr: &str, type_: PathType) -> Option<Entry> {
        self.defs
            .iter()
            .rev()
            .filter(|loaded| loaded.def.abbr == abbr)
            .find_map(|loaded| {
                self.eval(loaded)
                    .filter(|e| type_.matches(Path::new(&e.expanded)))
            })
    }

    /// Resolve every distinct abbreviation to its winning entry, in first-seen
    /// load order.  For each abbreviation, candidates are evaluated from the
    /// last in load order and evaluation stops at the first survivor.
    pub fn resolve_all(&self, type_: PathType) -> Vec<Entry> {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut order: Vec<&str> = Vec::new();
        for loaded in self.defs {
            if seen.insert(loaded.def.abbr.as_str()) {
                order.push(loaded.def.abbr.as_str());
            }
        }
        order
            .into_iter()
            .filter_map(|abbr| self.resolve_abbr(abbr, type_))
            .collect()
    }
}
