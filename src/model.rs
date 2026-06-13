use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathType {
    Directory,
    File,
    Existent,
}

impl PathType {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "directory" | "dir" | "d" => Ok(Self::Directory),
            "file" | "f" => Ok(Self::File),
            "existent" | "exist" | "e" => Ok(Self::Existent),
            _ => Err(format!("invalid type {s:?}")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Existent => "existent",
        }
    }

    pub fn matches(self, path: &Path) -> bool {
        match self {
            Self::Directory => path.is_dir(),
            Self::File => path.is_file(),
            Self::Existent => path.exists(),
        }
    }

    /// Default type for a definition without an explicit `type`.
    pub fn infer(path: &str) -> Self {
        if path.ends_with('/') {
            Self::Directory
        } else {
            Self::Existent
        }
    }
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Format {
    Tsv,
    Json,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum SortBy {
    Abbr,
    Path,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum CacheTarget {
    Shell,
}

impl SortBy {
    pub fn field(self) -> &'static str {
        match self {
            Self::Abbr => "abbr",
            Self::Path => "path",
        }
    }
}

/// A `[[path]]` table as written in a definition file.
#[derive(Debug, Deserialize)]
pub struct RawDef {
    pub abbr: String,
    pub path: String,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default, rename = "type")]
    pub type_: Option<String>,
}

/// A whole definition file.
#[derive(Debug, Default, Deserialize)]
pub struct DefFile {
    #[serde(default)]
    pub vars: toml::Table,
    #[serde(default, rename = "path")]
    pub paths: Vec<RawDef>,
}

/// A rendered, existence-checked entry ready for output.
#[derive(Debug)]
pub struct Entry {
    pub abbr: String,
    pub desc: Option<String>,
    /// Absolute path with `~` and templates expanded; a trailing slash from
    /// the definition is preserved.
    pub expanded: String,
    pub type_: PathType,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_aliases() {
        for s in ["directory", "dir", "d"] {
            assert_eq!(PathType::parse(s), Ok(PathType::Directory));
        }
        for s in ["file", "f"] {
            assert_eq!(PathType::parse(s), Ok(PathType::File));
        }
        for s in ["existent", "exist", "e"] {
            assert_eq!(PathType::parse(s), Ok(PathType::Existent));
        }
        assert!(PathType::parse("dirs").is_err());
    }

    #[test]
    fn type_inference() {
        assert_eq!(PathType::infer("~/src/"), PathType::Directory);
        assert_eq!(PathType::infer("~/init.el"), PathType::Existent);
    }
}
