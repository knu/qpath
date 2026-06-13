use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

struct Sandbox {
    home: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Sandbox {
            home: TempDir::new().unwrap(),
        }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn config_dir(&self) -> PathBuf {
        self.home().join(".config/qpath")
    }

    fn write_config(&self, rel: &str, content: &str) {
        let path = self.config_dir().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn read_config(&self, rel: &str) -> String {
        fs::read_to_string(self.config_dir().join(rel)).unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_qpath"))
            .args(args)
            .env_clear()
            .env("HOME", self.home())
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "qpath {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn fail(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            !out.status.success(),
            "qpath {args:?} unexpectedly succeeded"
        );
        String::from_utf8(out.stderr).unwrap()
    }
}

fn basic_sandbox() -> Sandbox {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home().join("src/github.com")).unwrap();
    fs::write(sb.home().join("init.el"), "").unwrap();
    sb.write_config(
        "paths.toml",
        r#"
[[path]]
abbr = "gh"
path = "~/src/github.com/"
desc = "GitHub"

[[path]]
abbr = "i"
path = "~/init.el"
type = "file"

[[path]]
abbr = "missing"
path = "~/nonexistent"
"#,
    );
    sb
}

#[test]
fn ls_tsv() {
    let sb = basic_sandbox();
    let home = sb.home().display().to_string();
    let out = sb.ok(&["ls"]);
    assert_eq!(
        out,
        format!(
            "gh\tGitHub\t{home}/src/github.com/\t~/src/github.com/\n\
             i\t~/init.el\t{home}/init.el\t~/init.el\n"
        )
    );
    // `list` is an alias.
    assert_eq!(sb.ok(&["list"]), out);
}

#[test]
fn ls_shell_path_escaping() {
    let sb = Sandbox::new();
    let home = sb.home().display().to_string();
    fs::create_dir_all(sb.home().join("Library/Application Support")).unwrap();
    sb.write_config(
        "paths.toml",
        "[[path]]\nabbr = \"as\"\npath = \"~/Library/Application Support/\"\n",
    );
    assert_eq!(
        sb.ok(&["ls"]),
        format!(
            "as\t~/Library/Application Support/\t{home}/Library/Application Support/\t~/Library/Application\\ Support/\n"
        )
    );
}

#[test]
fn ls_type_filter() {
    let sb = basic_sandbox();
    let home = sb.home().display().to_string();
    let out = sb.ok(&["ls", "--type", "directory"]);
    assert_eq!(
        out,
        format!("gh\tGitHub\t{home}/src/github.com/\t~/src/github.com/\n")
    );
    let out = sb.ok(&["ls", "--type", "f"]);
    assert_eq!(out, format!("i\t~/init.el\t{home}/init.el\t~/init.el\n"));
}

#[test]
fn show_exact_abbr() {
    let sb = basic_sandbox();
    let home = sb.home().display().to_string();

    // Exact match only; no prefix matching.
    let out = sb.ok(&["show", "gh"]);
    assert_eq!(
        out,
        format!("gh\tGitHub\t{home}/src/github.com/\t~/src/github.com/\n")
    );

    // --type and --format apply just like ls.
    let out = sb.ok(&["show", "i", "--format", "json"]);
    let items: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["abbr"], "i");

    // A type mismatch filters the entry out, leaving nothing to show.
    let err = sb.fail(&["show", "gh", "--type", "f"]);
    assert!(err.contains("not found"), "{err}");

    // An unknown abbreviation is not found.
    let err = sb.fail(&["show", "nope"]);
    assert!(err.contains("not found"), "{err}");

    // A defined but nonexistent path is filtered by the default type.
    let err = sb.fail(&["show", "missing"]);
    assert!(err.contains("not found"), "{err}");
}

#[test]
fn ls_json_and_expand() {
    let sb = basic_sandbox();
    let home = sb.home().display().to_string();

    let out = sb.ok(&["ls", "--format", "json"]);
    let items: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        items,
        serde_json::json!([
            {
                "abbr": "gh",
                "desc": "GitHub",
                "path": format!("{home}/src/github.com/"),
                "shell_path": "~/src/github.com/",
                "type": "directory"
            },
            {
                "abbr": "i",
                "desc": "~/init.el",
                "path": format!("{home}/init.el"),
                "shell_path": "~/init.el",
                "type": "file"
            }
        ])
    );

    let out = sb.ok(&["ls", "--expand"]);
    assert_eq!(
        out,
        format!(
            "gh\tGitHub\t{home}/src/github.com/\t{home}/src/github.com/\n\
             i\t{home}/init.el\t{home}/init.el\t{home}/init.el\n"
        )
    );
}

#[test]
fn ls_templates_vars_and_glob() {
    let sb = Sandbox::new();
    for v in ["3.9", "3.14"] {
        fs::create_dir_all(sb.home().join(format!("lib/python{v}/site-packages"))).unwrap();
    }
    sb.write_config(
        "paths.d/python.toml",
        r#"
[vars]
py_lib = "~/lib/"

[[path]]
abbr = "pl"
path = "{{ py_lib }}"
desc = "Python libs"

[[path]]
abbr = "sp"
path = "{{ '~/lib/python3.[0-9]*/site-packages/' | glob | vsort | last }}"
desc = "site-packages"
type = "directory"
"#,
    );
    let home = sb.home().display().to_string();
    let out = sb.ok(&["ls"]);
    assert_eq!(
        out,
        format!(
            "pl\tPython libs\t{home}/lib/\t~/lib/\n\
             sp\tsite-packages\t{home}/lib/python3.14/site-packages/\t~/lib/python3.14/site-packages/\n"
        )
    );
}

#[test]
fn ls_shell_filter() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home().join("shdir")).unwrap();
    sb.write_config(
        "paths.toml",
        r#"
[[path]]
abbr = "sd"
path = "{{ 'echo $HOME/shdir' | shell }}/"
desc = "Shell dir"

[[path]]
abbr = "bad"
path = "{{ 'exit 1' | shell }}"
"#,
    );
    let home = sb.home().display().to_string();
    let out = sb.run(&["ls"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("sd\tShell dir\t{home}/shdir/\t~/shdir/\n")
    );
    // The failing command is reported as a warning and the entry is skipped.
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains("bad"), "{stderr}");
}

#[test]
fn ls_shell_filter_cache_ttl() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.home().join("shdir")).unwrap();
    sb.write_config(
        "paths.toml",
        r#"
[[path]]
abbr = "sd"
path = "{{ 'echo x >> $HOME/counter; echo $HOME/shdir' | shell(cache_ttl=3600) }}/"
"#,
    );
    let home = sb.home().display().to_string();
    let expected = format!("sd\t~/shdir/\t{home}/shdir/\t~/shdir/\n");
    assert_eq!(sb.ok(&["ls"]), expected);
    assert_eq!(sb.ok(&["ls"]), expected);
    // The second run was served from the cache.
    assert_eq!(
        fs::read_to_string(sb.home().join("counter")).unwrap(),
        "x\n"
    );
    let cached = fs::read_dir(sb.home().join(".cache/qpath/shell"))
        .unwrap()
        .count();
    assert_eq!(cached, 1);

    // Clearing the shell cache makes the next run execute the command again.
    sb.ok(&["cache", "clear", "shell"]);
    assert!(!sb.home().join(".cache/qpath/shell").exists());
    assert_eq!(sb.ok(&["ls"]), expected);
    assert_eq!(
        fs::read_to_string(sb.home().join("counter")).unwrap(),
        "x\nx\n"
    );

    // Clearing everything removes the whole cache directory and is
    // idempotent.
    sb.ok(&["cache", "clear"]);
    assert!(!sb.home().join(".cache/qpath").exists());
    sb.ok(&["cache", "clear"]);

    // Unknown cache names are rejected.
    sb.fail(&["cache", "clear", "bogus"]);
}

#[test]
fn add_creates_sorted_file() {
    let sb = Sandbox::new();
    sb.ok(&["add", "z", "~/z/"]);
    sb.ok(&["add", "a", "~/a.txt", "--desc", "A file", "--type", "f"]);
    let text = sb.read_config("paths.toml");
    let a = text.find("abbr = \"a\"").unwrap();
    let z = text.find("abbr = \"z\"").unwrap();
    assert!(a < z, "entries are sorted by abbr:\n{text}");
    assert!(text.contains("desc = \"A file\""));
    assert!(text.contains("type = \"file\""));

    let doc: toml::Table = toml::from_str(&text).unwrap();
    assert_eq!(doc["path"].as_array().unwrap().len(), 2);
}

#[test]
fn add_duplicate_handling() {
    let sb = Sandbox::new();
    sb.ok(&["add", "gh", "~/src/github.com/", "--desc", "GitHub"]);

    let err = sb.fail(&["add", "gh", "~/elsewhere/"]);
    assert!(err.contains("already exists"), "{err}");

    sb.ok(&["add", "gh", "~/src/gitlab.com/", "--overwrite"]);
    let text = sb.read_config("paths.toml");
    assert!(text.contains("path = \"~/src/gitlab.com/\""), "{text}");
    // desc is preserved when --desc is not given.
    assert!(text.contains("desc = \"GitHub\""), "{text}");
}

#[test]
fn add_warns_when_defined_elsewhere() {
    let sb = Sandbox::new();
    sb.write_config(
        "paths.d/common.toml",
        "[[path]]\nabbr = \"gh\"\npath = \"~/src/github.com/\"\n",
    );
    // Adding to the default file succeeds but warns about the other entry.
    let out = sb.run(&["add", "gh", "~/other/"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains("paths.d/common.toml"), "{stderr}");
    assert!(
        sb.read_config("paths.toml").contains("path = \"~/other/\""),
        "the entry was added to the target file"
    );
}

#[test]
fn add_expand_saves_absolute_path() {
    let sb = Sandbox::new();
    let home = sb.home().display().to_string();
    sb.ok(&["add", "gh", "~/src/github.com/", "--expand"]);
    let text = sb.read_config("paths.toml");
    assert!(
        text.contains(&format!("path = \"{home}/src/github.com/\"")),
        "{text}"
    );
}

#[test]
fn update_entry() {
    let sb = Sandbox::new();
    sb.ok(&["add", "gh", "~/src/github.com/", "--desc", "GitHub"]);

    // Replace the path; desc is preserved when not given.
    sb.ok(&["update", "gh", "~/src/gitlab.com/"]);
    let text = sb.read_config("paths.toml");
    assert!(text.contains("path = \"~/src/gitlab.com/\""), "{text}");
    assert!(text.contains("desc = \"GitHub\""), "{text}");

    // Omitting the path updates only desc/type.
    sb.ok(&["update", "gh", "--desc", "GitLab", "--type", "d"]);
    let text = sb.read_config("paths.toml");
    assert!(text.contains("path = \"~/src/gitlab.com/\""), "{text}");
    assert!(text.contains("desc = \"GitLab\""), "{text}");
    assert!(text.contains("type = \"directory\""), "{text}");

    // Updating a missing abbreviation is an error.
    let err = sb.fail(&["update", "nope", "~/x/"]);
    assert!(err.contains("not found"), "{err}");
}

#[test]
fn update_only_edits_target_file() {
    let sb = Sandbox::new();
    sb.write_config(
        "paths.d/common.toml",
        "[[path]]\nabbr = \"gh\"\npath = \"~/src/github.com/\"\n",
    );
    // update only edits the target file (default paths.toml); an entry
    // defined elsewhere is not found, but the error points at the other file.
    let err = sb.fail(&["update", "gh", "~/other/"]);
    assert!(err.contains("not found"), "{err}");
    assert!(err.contains("paths.d/common.toml"), "{err}");
    // The other file is left untouched.
    assert!(
        sb.read_config("paths.d/common.toml")
            .contains("path = \"~/src/github.com/\""),
        "other file unchanged"
    );
}

#[test]
fn rename_entry() {
    let sb = Sandbox::new();
    sb.ok(&["add", "gh", "~/src/github.com/"]);
    sb.ok(&["rename", "gh", "hub"]);
    let text = sb.read_config("paths.toml");
    assert!(text.contains("abbr = \"hub\""), "{text}");
    assert!(!text.contains("abbr = \"gh\""), "{text}");

    let err = sb.fail(&["rename", "nope", "x"]);
    assert!(err.contains("not found"), "{err}");
}

#[test]
fn rename_collision() {
    let sb = Sandbox::new();
    sb.ok(&["add", "a", "~/a/"]);
    sb.write_config(
        "paths.d/common.toml",
        "[[path]]\nabbr = \"b\"\npath = \"~/b/\"\n",
    );
    let err = sb.fail(&["rename", "a", "b"]);
    assert!(err.contains("already exists"), "{err}");
    assert!(err.contains("paths.d/common.toml"), "{err}");
}

#[test]
fn rm_entry() {
    let sb = Sandbox::new();
    sb.ok(&["add", "a", "~/a/"]);
    sb.ok(&["add", "b", "~/b/"]);
    sb.ok(&["rm", "a"]);
    let text = sb.read_config("paths.toml");
    assert!(!text.contains("abbr = \"a\""), "{text}");
    assert!(text.contains("abbr = \"b\""), "{text}");

    let err = sb.fail(&["rm", "a"]);
    assert!(err.contains("not found"), "{err}");

    // `remove` is an alias.
    sb.ok(&["remove", "b"]);
    assert!(!sb.read_config("paths.toml").contains("abbr = \"b\""));
}

#[test]
fn format_sorts_file() {
    let sb = Sandbox::new();
    // Trailing spaces and extra blank lines should be tidied away.
    sb.write_config(
        "paths.toml",
        "# header  \n\n\n[[path]]   \nabbr = \"z\"  \npath = \"~/z/\"\n\n\n\n[[path]]\nabbr = \"a\"\npath = \"~/a/\"\n\n\n",
    );
    // `fmt` is an alias and sorts by abbr by default.
    sb.ok(&["fmt"]);
    let text = sb.read_config("paths.toml");
    assert_eq!(
        text,
        "# header\n\n[[path]]\nabbr = \"a\"\npath = \"~/a/\"\n\n[[path]]\nabbr = \"z\"\npath = \"~/z/\"\n",
        "sorted, header on top, whitespace tidied"
    );

    // --sort-by path reorders by path instead.  Pass --file as an absolute
    // path since it is resolved against the working directory, not HOME.
    sb.write_config(
        "paths.d/x.toml",
        "[[path]]\nabbr = \"a\"\npath = \"~/z/\"\n\n[[path]]\nabbr = \"z\"\npath = \"~/a/\"\n",
    );
    let x_file = sb.config_dir().join("paths.d/x.toml");
    sb.ok(&[
        "format",
        "--file",
        x_file.to_str().unwrap(),
        "--sort-by",
        "path",
    ]);
    let text = sb.read_config("paths.d/x.toml");
    let za = text.find("path = \"~/a/\"").unwrap();
    let zz = text.find("path = \"~/z/\"").unwrap();
    assert!(za < zz, "entries are sorted by path:\n{text}");

    // A missing file is an error.
    let missing = sb.config_dir().join("paths.d/missing.toml");
    let err = sb.fail(&["format", "--file", missing.to_str().unwrap()]);
    assert!(err.contains("does not exist"), "{err}");
}

#[test]
fn edit_preserves_comments() {
    let sb = Sandbox::new();
    sb.write_config(
        "paths.toml",
        "# My paths\n\n[[path]]\n# GitHub checkout root\nabbr = \"gh\"\npath = \"~/src/github.com/\"\n",
    );
    sb.ok(&["add", "a", "~/a/"]);
    let text = sb.read_config("paths.toml");
    assert!(text.contains("# My paths"), "{text}");
    assert!(text.contains("# GitHub checkout root"), "{text}");
}
