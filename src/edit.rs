use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

pub fn open_doc(file: &Path) -> Result<DocumentMut> {
    if !file.exists() {
        return Ok(DocumentMut::new());
    }
    fs::read_to_string(file)
        .with_context(|| format!("cannot read {}", file.display()))?
        .parse()
        .with_context(|| format!("cannot parse {}", file.display()))
}

pub fn path_tables(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables> {
    doc.entry("path")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .context("'path' is not an array of tables")
}

pub fn find_indices(tables: &ArrayOfTables, abbr: &str) -> Vec<usize> {
    tables
        .iter()
        .enumerate()
        .filter(|(_, t)| t.get("abbr").and_then(|v| v.as_str()) == Some(abbr))
        .map(|(i, _)| i)
        .collect()
}

pub fn sort_tables(tables: &mut ArrayOfTables, field: &str) {
    let mut sorted: Vec<Table> = tables.iter().cloned().collect();
    sorted.sort_by_key(|t| {
        t.get(field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    });
    tables.clear();
    for t in sorted {
        tables.push(t);
    }
}

pub fn save(file: &Path, doc: &DocumentMut) -> Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    fs::write(file, doc.to_string()).with_context(|| format!("cannot write {}", file.display()))
}
