mod commands;
mod dirs;
mod edit;
mod load;
mod model;
mod template;
mod vsort;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::dirs::BaseDirs;
use crate::model::{CacheTarget, Format, PathType, SortBy};

#[derive(Parser)]
#[command(
    name = "qpath",
    version,
    about = "Register and list frequently used paths"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List registered paths
    #[command(alias = "list")]
    Ls {
        /// Filter by path type (directory, file, existent)
        #[arg(long = "type", value_name = "TYPE", value_parser = PathType::parse,
              default_value = "existent")]
        type_: PathType,
        /// Output format
        #[arg(long, value_enum, default_value = "tsv")]
        format: Format,
        /// Output expanded absolute paths
        #[arg(long)]
        expand: bool,
    },
    /// Add or update a path definition
    Add {
        abbr: String,
        path: String,
        /// Path type (directory, file, existent)
        #[arg(long = "type", value_name = "TYPE", value_parser = PathType::parse)]
        type_: Option<PathType>,
        /// Description
        #[arg(long)]
        desc: Option<String>,
        /// Definition file to edit
        #[arg(long)]
        file: Option<PathBuf>,
        /// Sort entries by this field after editing
        #[arg(long, value_enum, default_value = "abbr")]
        sort_by: SortBy,
        /// Update an existing entry with the same abbreviation
        #[arg(long)]
        overwrite: bool,
        /// Save the path as an expanded absolute path
        #[arg(long)]
        expand: bool,
    },
    /// Update an existing path definition
    Update {
        abbr: String,
        path: Option<String>,
        /// Path type (directory, file, existent)
        #[arg(long = "type", value_name = "TYPE", value_parser = PathType::parse)]
        type_: Option<PathType>,
        /// Description
        #[arg(long)]
        desc: Option<String>,
        /// Definition file to edit
        #[arg(long)]
        file: Option<PathBuf>,
        /// Sort entries by this field after editing
        #[arg(long, value_enum, default_value = "abbr")]
        sort_by: SortBy,
        /// Save the path as an expanded absolute path
        #[arg(long)]
        expand: bool,
    },
    /// Rename a path abbreviation
    Rename {
        abbr: String,
        new_abbr: String,
        /// Definition file to edit
        #[arg(long)]
        file: Option<PathBuf>,
        /// Sort entries by this field after editing
        #[arg(long, value_enum, default_value = "abbr")]
        sort_by: SortBy,
    },
    /// Remove a path definition
    #[command(alias = "remove")]
    Rm {
        abbr: String,
        /// Definition file to edit
        #[arg(long)]
        file: Option<PathBuf>,
        /// Sort entries by this field after editing
        #[arg(long, value_enum, default_value = "abbr")]
        sort_by: SortBy,
    },
    /// Sort and reformat a definition file
    #[command(alias = "fmt")]
    Format {
        /// Definition file to edit
        #[arg(long)]
        file: Option<PathBuf>,
        /// Sort entries by this field
        #[arg(long, value_enum, default_value = "abbr")]
        sort_by: SortBy,
    },
    /// Manage cache files
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
}

#[derive(Subcommand)]
enum CacheCommand {
    /// Remove cache files
    Clear {
        /// Cache to clear; all caches when omitted
        #[arg(value_enum)]
        target: Option<CacheTarget>,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("qpath: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let dirs = BaseDirs::from_env()?;
    match cli.command {
        Command::Ls {
            type_,
            format,
            expand,
        } => commands::ls(&dirs, type_, format, expand),
        Command::Add {
            abbr,
            path,
            type_,
            desc,
            file,
            sort_by,
            overwrite,
            expand,
        } => commands::add(
            &dirs,
            commands::AddOpts {
                abbr,
                path,
                type_,
                desc,
                file,
                sort_by,
                overwrite,
                expand,
            },
        ),
        Command::Update {
            abbr,
            path,
            type_,
            desc,
            file,
            sort_by,
            expand,
        } => commands::update(
            &dirs,
            commands::UpdateOpts {
                abbr,
                path,
                type_,
                desc,
                file,
                sort_by,
                expand,
            },
        ),
        Command::Rename {
            abbr,
            new_abbr,
            file,
            sort_by,
        } => commands::rename(&dirs, &abbr, &new_abbr, file, sort_by),
        Command::Rm {
            abbr,
            file,
            sort_by,
        } => commands::rm(&dirs, &abbr, file, sort_by),
        Command::Format { file, sort_by } => commands::format(&dirs, file, sort_by),
        Command::Cache {
            command: CacheCommand::Clear { target },
        } => commands::cache_clear(&dirs, target),
    }
}
