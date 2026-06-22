use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use toml_edit::{Table, value};

use crate::dirs::{BaseDirs, expand_tilde, shorten_home};
use crate::edit;
use crate::load::{self, Definitions};
use crate::model::{CacheTarget, Entry, Format, PathType, SortBy};

pub fn ls(dirs: &BaseDirs, type_: PathType, format: Format, expand: bool) -> Result<()> {
    let definitions = load::load(&dirs.qpath_config_dir())?;
    let mut entries = load::Resolver::new(dirs, &definitions).resolve_all(type_);
    entries.sort_by(|a, b| a.abbr.cmp(&b.abbr));
    print_entries(dirs, &entries, format, expand)
}

pub fn show(
    dirs: &BaseDirs,
    abbr: &str,
    type_: PathType,
    format: Format,
    expand: bool,
) -> Result<()> {
    let definitions = load::load(&dirs.qpath_config_dir())?;
    match load::Resolver::new(dirs, &definitions).resolve_abbr(abbr, type_) {
        Some(e) => print_entries(dirs, &[e], format, expand),
        None => bail!("'{abbr}' not found"),
    }
}

fn print_entries(dirs: &BaseDirs, entries: &[Entry], format: Format, expand: bool) -> Result<()> {
    match format {
        Format::Tsv => {
            for e in entries {
                let display = display_path(e, expand, dirs);
                let desc = e.desc.as_deref().unwrap_or(&display);
                println!(
                    "{}\t{}\t{}\t{}",
                    sanitize(&e.abbr),
                    sanitize(desc),
                    sanitize(&e.expanded),
                    sanitize(&shell_path(&display))
                );
            }
        }
        Format::Json => {
            let items: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    let display = display_path(e, expand, dirs);
                    let desc = e.desc.clone().unwrap_or_else(|| display.clone());
                    serde_json::json!({
                        "abbr": e.abbr,
                        "desc": desc,
                        "path": e.expanded,
                        "shell_path": shell_path(&display),
                        "source": e.source.to_string_lossy(),
                        "type": e.type_.name(),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
    }
    Ok(())
}

/// Quote a path for direct insertion into a shell command line.  A leading
/// `~/` is left unquoted so tilde expansion still applies.
fn shell_path(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("~/{}", shell_escape(rest)),
        None => shell_escape(path),
    }
}

/// Backslash-escape ASCII characters that are not safe unquoted in sh-like
/// shells.  Non-ASCII characters are passed through.
fn shell_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() && !c.is_ascii_alphanumeric() && !matches!(c, '_' | '-' | '.' | '/') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub struct AddOpts {
    pub abbr: String,
    pub path: String,
    pub type_: Option<PathType>,
    pub desc: Option<String>,
    pub file: Option<PathBuf>,
    pub sort_by: SortBy,
    pub overwrite: bool,
    pub expand: bool,
}

pub fn add(dirs: &BaseDirs, opts: AddOpts) -> Result<()> {
    let config_dir = dirs.qpath_config_dir();
    let target = resolve_target(opts.file.as_deref(), dirs, &config_dir);
    let definitions = load::load(&config_dir)?;
    let elsewhere = first_elsewhere(&definitions, &opts.abbr, &target);

    // The same abbreviation living in another file is allowed, but worth
    // pointing out since this edit will not touch that entry.
    if let Some(file) = elsewhere {
        eprintln!(
            "qpath: warning: '{}' is also defined in {}",
            opts.abbr,
            file.display()
        );
    }

    let mut doc = edit::open_doc(&target)?;
    let tables = edit::path_tables(&mut doc)?;
    let saved = normalize_save_path(&opts.path, &dirs.home, opts.expand);

    // With --overwrite, replace the last existing entry if there is one;
    // otherwise (and by default) just append.  Duplicates are allowed because
    // later definitions win at load time.
    let existing = if opts.overwrite {
        edit::find_indices(tables, &opts.abbr).pop()
    } else {
        None
    };
    if let Some(index) = existing {
        let t = tables.get_mut(index).unwrap();
        t["path"] = value(&saved);
        apply_optional_fields(t, opts.desc.as_deref(), opts.type_);
    } else {
        let mut t = Table::new();
        t["abbr"] = value(&opts.abbr);
        t["path"] = value(&saved);
        apply_optional_fields(&mut t, opts.desc.as_deref(), opts.type_);
        tables.push(t);
    }

    edit::sort_tables(tables, opts.sort_by.field());
    edit::save(&target, &doc)
}

pub struct UpdateOpts {
    pub abbr: String,
    pub path: Option<String>,
    pub type_: Option<PathType>,
    pub desc: Option<String>,
    pub file: Option<PathBuf>,
    pub sort_by: SortBy,
    pub expand: bool,
}

pub fn update(dirs: &BaseDirs, opts: UpdateOpts) -> Result<()> {
    let config_dir = dirs.qpath_config_dir();
    let definitions = load::load(&config_dir)?;
    let target = resolve_existing_target(
        opts.file.as_deref(),
        dirs,
        &config_dir,
        &definitions,
        &opts.abbr,
    )?;

    let mut doc = edit::open_doc(&target)?;
    let tables = edit::path_tables(&mut doc)?;
    // Update the last matching entry, since later definitions win.
    let index = match edit::find_indices(tables, &opts.abbr).pop() {
        Some(index) => index,
        None => {
            // Point at another file holding the same abbreviation, if any, so
            // the user knows where to look.
            match first_elsewhere(&definitions, &opts.abbr, &target) {
                Some(file) => bail!(
                    "'{}' not found in {}; it is defined in {}",
                    opts.abbr,
                    target.display(),
                    file.display()
                ),
                None => bail!("'{}' not found in {}", opts.abbr, target.display()),
            }
        }
    };

    let t = tables.get_mut(index).unwrap();
    if let Some(path) = &opts.path {
        t["path"] = value(normalize_save_path(path, &dirs.home, opts.expand));
    }
    apply_optional_fields(t, opts.desc.as_deref(), opts.type_);

    edit::sort_tables(tables, opts.sort_by.field());
    edit::save(&target, &doc)
}

/// Set `desc` and `type` on an existing entry when given, leaving them
/// untouched otherwise.
fn apply_optional_fields(t: &mut Table, desc: Option<&str>, type_: Option<PathType>) {
    if let Some(desc) = desc {
        t["desc"] = value(desc);
    }
    if let Some(type_) = type_ {
        t["type"] = value(type_.name());
    }
}

pub fn rename(
    dirs: &BaseDirs,
    abbr: &str,
    new_abbr: &str,
    file: Option<PathBuf>,
    sort_by: SortBy,
) -> Result<()> {
    let config_dir = dirs.qpath_config_dir();
    let definitions = load::load(&config_dir)?;
    let target = resolve_existing_target(file.as_deref(), dirs, &config_dir, &definitions, abbr)?;
    if let Some(loaded) = definitions.defs.iter().find(|d| d.def.abbr == new_abbr) {
        bail!("'{}' already exists in {}", new_abbr, loaded.file.display());
    }

    let mut doc = edit::open_doc(&target)?;
    let tables = edit::path_tables(&mut doc)?;
    let index = last_index(tables, abbr, &target)?;
    tables.get_mut(index).unwrap()["abbr"] = value(new_abbr);
    edit::sort_tables(tables, sort_by.field());
    edit::save(&target, &doc)
}

pub fn rm(dirs: &BaseDirs, abbr: &str, file: Option<PathBuf>, sort_by: SortBy) -> Result<()> {
    let config_dir = dirs.qpath_config_dir();
    let definitions = load::load(&config_dir)?;
    let target = resolve_existing_target(file.as_deref(), dirs, &config_dir, &definitions, abbr)?;

    let mut doc = edit::open_doc(&target)?;
    let tables = edit::path_tables(&mut doc)?;
    let index = last_index(tables, abbr, &target)?;
    tables.remove(index);
    edit::sort_tables(tables, sort_by.field());
    edit::save(&target, &doc)
}

pub fn format(dirs: &BaseDirs, file: Option<PathBuf>, sort_by: SortBy) -> Result<()> {
    let config_dir = dirs.qpath_config_dir();
    let target = resolve_target(file.as_deref(), dirs, &config_dir);
    if !target.exists() {
        bail!("{} does not exist", target.display());
    }

    let mut doc = edit::open_doc(&target)?;
    let tables = edit::path_tables(&mut doc)?;
    edit::sort_tables(tables, sort_by.field());
    edit::save(&target, &doc)
}

pub fn cache_clear(dirs: &BaseDirs, target: Option<CacheTarget>) -> Result<()> {
    let dir = match target {
        Some(CacheTarget::Shell) => dirs.qpath_cache_dir().join("shell"),
        None => dirs.qpath_cache_dir(),
    };
    match fs::remove_dir_all(&dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("cannot remove {}", dir.display()))
        }
        _ => Ok(()),
    }
}

/// Index of the last entry with this abbreviation, since later definitions win.
fn last_index(tables: &toml_edit::ArrayOfTables, abbr: &str, target: &Path) -> Result<usize> {
    edit::find_indices(tables, abbr)
        .pop()
        .with_context(|| format!("'{}' not found in {}", abbr, target.display()))
}

fn display_path(entry: &Entry, expand: bool, dirs: &BaseDirs) -> String {
    if expand {
        entry.expanded.clone()
    } else {
        shorten_home(&entry.expanded, &dirs.home)
    }
}

fn sanitize(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

fn resolve_target(file: Option<&Path>, dirs: &BaseDirs, config_dir: &Path) -> PathBuf {
    match file {
        Some(f) => {
            let expanded = expand_tilde(&f.to_string_lossy(), &dirs.home);
            std::path::absolute(&expanded).unwrap_or_else(|_| PathBuf::from(expanded))
        }
        None => config_dir.join("paths.toml"),
    }
}

fn resolve_existing_target(
    file: Option<&Path>,
    dirs: &BaseDirs,
    config_dir: &Path,
    definitions: &Definitions,
    abbr: &str,
) -> Result<PathBuf> {
    match file {
        Some(_) => Ok(resolve_target(file, dirs, config_dir)),
        None => definitions
            .defs
            .iter()
            .rev()
            .find(|loaded| loaded.def.abbr == abbr)
            .map(|loaded| loaded.file.clone())
            .with_context(|| format!("'{}' not found", abbr)),
    }
}

fn first_elsewhere<'a>(
    definitions: &'a Definitions,
    abbr: &str,
    target: &Path,
) -> Option<&'a PathBuf> {
    definitions
        .defs
        .iter()
        .find(|d| d.def.abbr == abbr && d.file != target)
        .map(|d| &d.file)
}

/// Normalize a user-supplied path for saving.  Template expressions are
/// preserved as written; otherwise the path is made absolute and, unless
/// `expand` is set, shortened under the home directory to `~/...`.
fn normalize_save_path(input: &str, home: &Path, expand: bool) -> String {
    if input.contains("{{") || input.contains("{%") {
        return input.to_string();
    }
    let trailing_slash = input.ends_with('/') && input != "/";
    let expanded = expand_tilde(input, home);
    let absolute = std::path::absolute(&expanded)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(expanded);
    let mut saved = if expand {
        absolute
    } else {
        shorten_home(&absolute, home)
    };
    if trailing_slash && !saved.ends_with('/') {
        saved.push('/');
    }
    saved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_separators() {
        assert_eq!(sanitize("a\tb\rc\nd"), "a b c d");
        assert_eq!(sanitize("plain"), "plain");
    }

    #[test]
    fn shell_path_quoting() {
        assert_eq!(shell_path("~/src/github.com/"), "~/src/github.com/");
        assert_eq!(
            shell_path("~/Library/Application Support/"),
            "~/Library/Application\\ Support/"
        );
        assert_eq!(shell_path("/opt/foo bar/'x'/"), "/opt/foo\\ bar/\\'x\\'/");
        // A tilde not followed by a slash is escaped, not expandable.
        assert_eq!(shell_path("~foo"), "\\~foo");
        // Non-ASCII characters need no quoting.
        assert_eq!(shell_path("~/写真/"), "~/写真/");
    }

    #[test]
    fn save_path_normalization() {
        let home = Path::new("/home/u");
        assert_eq!(normalize_save_path("~/src/", home, false), "~/src/");
        assert_eq!(normalize_save_path("~/src/", home, true), "/home/u/src/");
        assert_eq!(
            normalize_save_path("/home/u/init.el", home, false),
            "~/init.el"
        );
        assert_eq!(normalize_save_path("/etc/hosts", home, false), "/etc/hosts");
        assert_eq!(normalize_save_path("/etc/", home, true), "/etc/");
        assert_eq!(
            normalize_save_path("{{ config_home }}/Code/User/", home, true),
            "{{ config_home }}/Code/User/"
        );
    }
}
