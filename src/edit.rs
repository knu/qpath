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

    // toml_edit keeps each table's original document position and leading
    // decoration (comments and blank lines), so reordering the logical vector
    // alone has no visible effect.  Reassign positions and normalize the
    // leading whitespace: keep the document header (everything up to the last
    // blank line of the first table's prefix) on top, keep each entry's own
    // preceding comment, and separate entries with a single blank line.
    let (header, first_own) = split_header(sorted.first().map(prefix_of).unwrap_or_default());
    if let Some(first) = sorted.first_mut() {
        first.decor_mut().set_prefix(first_own);
    }
    sorted.sort_by_key(|t| {
        t.get(field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    });

    let mut rebuilt = ArrayOfTables::new();
    for (i, mut t) in sorted.into_iter().enumerate() {
        let comment = prefix_of(&t).trim_start_matches('\n').to_string();
        let prefix = if i == 0 {
            format!("{header}{comment}")
        } else {
            format!("\n{comment}")
        };
        t.decor_mut().set_prefix(prefix);
        t.set_position(Some(i as isize));
        rebuilt.push(t);
    }
    *tables = rebuilt;
}

fn prefix_of(t: &Table) -> String {
    t.decor()
        .prefix()
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string()
}

/// Split a leading decoration into the document header (everything up to and
/// including the last blank line) and the entry's own preceding comment.
fn split_header(prefix: String) -> (String, String) {
    match prefix.rfind("\n\n") {
        Some(i) => (prefix[..i + 2].to_string(), prefix[i + 2..].to_string()),
        None => (String::new(), prefix),
    }
}

/// Tidy serialized TOML: strip trailing whitespace from each line, collapse
/// runs of blank lines to a single blank line, drop leading blank lines, and
/// end with exactly one newline.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_blank = false;
    let mut wrote_content = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            pending_blank = wrote_content;
            continue;
        }
        if pending_blank {
            out.push('\n');
            pending_blank = false;
        }
        out.push_str(line);
        out.push('\n');
        wrote_content = true;
    }
    out
}

pub fn save(file: &Path, doc: &DocumentMut) -> Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    fs::write(file, tidy(&doc.to_string()))
        .with_context(|| format!("cannot write {}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::tidy;

    #[test]
    fn tidy_normalizes_whitespace() {
        let input = "# header  \n\n\n[[path]]   \nabbr = \"a\"  \npath = \"~/a/\"\n\n\n\n[[path]]\nabbr = \"b\"\npath = \"~/b/\"\n\n\n";
        let want = "# header\n\n[[path]]\nabbr = \"a\"\npath = \"~/a/\"\n\n[[path]]\nabbr = \"b\"\npath = \"~/b/\"\n";
        assert_eq!(tidy(input), want);
    }

    #[test]
    fn tidy_drops_leading_blank_lines() {
        assert_eq!(
            tidy("\n\n[[path]]\nabbr = \"a\"\n"),
            "[[path]]\nabbr = \"a\"\n"
        );
    }

    #[test]
    fn tidy_empty_stays_empty() {
        assert_eq!(tidy(""), "");
        assert_eq!(tidy("\n\n"), "");
    }
}
