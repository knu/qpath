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

/// Render templates, apply type defaults, and drop entries whose paths do not
/// exist or do not match their type.  Render and type errors are reported to
/// stderr and the entry is skipped.
pub fn resolve(dirs: &BaseDirs, definitions: &Definitions) -> Vec<Entry> {
    let env = template::build_env(dirs, &definitions.vars);
    let mut entries = Vec::new();
    for loaded in &definitions.defs {
        let def = &loaded.def;
        let warn = |msg: &dyn std::fmt::Display| {
            eprintln!(
                "qpath: warning: {}: {}: {msg}",
                loaded.file.display(),
                def.abbr
            );
        };
        let rendered = match env.render_str(&def.path, context! {}) {
            Ok(s) => s,
            Err(e) => {
                warn(&e);
                continue;
            }
        };
        let type_ = match &def.type_ {
            Some(s) => match PathType::parse(s) {
                Ok(t) => t,
                Err(e) => {
                    warn(&e);
                    continue;
                }
            },
            None => PathType::infer(&rendered),
        };
        let expanded = expand_tilde(&rendered, &dirs.home);
        if !type_.matches(Path::new(&expanded)) {
            continue;
        }
        entries.push(Entry {
            abbr: def.abbr.clone(),
            desc: def.desc.clone(),
            expanded,
            type_,
        });
    }
    entries
}
