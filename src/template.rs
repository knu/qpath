use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use minijinja::value::Kwargs;
use minijinja::{Environment, Error, ErrorKind, UndefinedBehavior, Value};
use serde::{Deserialize, Serialize};

use crate::dirs::{BaseDirs, expand_tilde};
use crate::vsort::version_cmp;

/// Build a MiniJinja environment with the built-in variables, user `[vars]`,
/// and the `glob` and `shell` filters.
pub fn build_env(dirs: &BaseDirs, vars: &toml::Table) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_global("home", dirs.home.display().to_string());
    env.add_global("config_home", dirs.config_home.display().to_string());
    env.add_global("data_home", dirs.data_home.display().to_string());
    env.add_global("cache_home", dirs.cache_home.display().to_string());
    env.add_global("state_home", dirs.state_home.display().to_string());
    env.add_global("os", std::env::consts::OS);
    env.add_global("arch", std::env::consts::ARCH);
    for (k, v) in vars {
        env.add_global(k.clone(), Value::from_serialize(v));
    }
    let home = dirs.home.clone();
    env.add_filter("glob", move |pattern: String| {
        glob_expand(&pattern, &home)
            .map_err(|e| Error::new(ErrorKind::InvalidOperation, e.to_string()))
    });
    let cache_dir = dirs.qpath_cache_dir().join("shell");
    env.add_filter(
        "shell",
        move |command: String, shell_arg: Option<String>, kwargs: Kwargs| {
            let cache_ttl: Option<u64> = kwargs.get("cache_ttl")?;
            kwargs.assert_all_used()?;
            let shell = shell_arg.as_deref().unwrap_or("/bin/sh");
            match cache_ttl {
                Some(secs) => {
                    shell_run_cached(shell, &command, &cache_dir, Duration::from_secs(secs))
                }
                None => shell_run(shell, &command),
            }
            .map_err(|e| Error::new(ErrorKind::InvalidOperation, e))
        },
    );
    env.add_filter("vsort", |items: Vec<String>| {
        let mut items = items;
        items.sort_by(|a, b| version_cmp(a, b));
        items
    });
    env
}

/// Run a command with `<shell> -c` and return its stdout with trailing newlines
/// stripped, like `$(...)` command substitution.
fn shell_run(shell: &str, command: &str) -> Result<String, String> {
    if shell.is_empty() {
        return Err("shell must not be empty".to_string());
    }
    let output = Command::new(shell)
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {shell}: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim_end();
        return Err(format!(
            "command failed with {shell} ({}): {command}{}{stderr}",
            output.status,
            if stderr.is_empty() { "" } else { ": " }
        ));
    }
    let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    stdout.truncate(stdout.trim_end_matches(['\r', '\n']).len());
    Ok(stdout)
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    shell: String,
    command: String,
    output: String,
}

/// Run a command through a per-command cache.  A cache file fresher than
/// `ttl` (by mtime) short-circuits execution; failures are never cached.
fn shell_run_cached(
    shell: &str,
    command: &str,
    cache_dir: &Path,
    ttl: Duration,
) -> Result<String, String> {
    let path = cache_dir.join(format!("{:016x}.json", fnv1a(&cache_key(shell, command))));
    if let Some(output) = read_cache(&path, shell, command, ttl) {
        return Ok(output);
    }
    let output = shell_run(shell, command)?;
    if let Err(e) = write_cache(&path, shell, command, &output) {
        eprintln!("qpath: warning: cannot write {}: {e}", path.display());
    }
    Ok(output)
}

fn read_cache(path: &Path, shell: &str, command: &str, ttl: Duration) -> Option<String> {
    let age = fs::metadata(path).ok()?.modified().ok()?.elapsed().ok()?;
    if age >= ttl {
        return None;
    }
    let entry: CacheEntry = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    // Guard against hash collisions; a mismatch is treated as a miss.
    (entry.shell == shell && entry.command == command).then_some(entry.output)
}

fn write_cache(path: &Path, shell: &str, command: &str, output: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let entry = CacheEntry {
        shell: shell.to_string(),
        command: command.to_string(),
        output: output.to_string(),
    };
    // Write to a per-process temporary file and rename so concurrent qpath
    // invocations never observe a partially written cache entry.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&tmp, serde_json::to_string(&entry)?)?;
    fs::rename(&tmp, path)
}

fn cache_key(shell: &str, command: &str) -> String {
    format!("{shell}\0{command}")
}

fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// Expand a glob pattern into matching paths.  Brace alternations such as
/// `{a,b}` are expanded first, then each alternative has its tilde expanded
/// and is globbed independently.  Matches within each alternative are sorted
/// lexically and the alternatives' results are concatenated in order; there
/// is no re-sort across alternatives.  Apply the `vsort` filter to sort the
/// combined result in version-fragment order.  A trailing slash restricts
/// matches to directories and is kept in the results.
fn glob_expand(pattern: &str, home: &Path) -> Result<Vec<String>, glob::PatternError> {
    let mut results = Vec::new();
    for alternative in brace_expand::brace_expand(pattern) {
        let expanded = expand_tilde(&alternative, home);
        let dirs_only = expanded.ends_with('/');
        let trimmed = expanded.trim_end_matches('/');
        let pat = if trimmed.is_empty() { "/" } else { trimmed };
        let mut matches: Vec<String> = Vec::new();
        for entry in glob::glob(pat)?.flatten() {
            if dirs_only && !entry.is_dir() {
                continue;
            }
            let mut s = entry.to_string_lossy().into_owned();
            if dirs_only {
                s.push('/');
            }
            matches.push(s);
        }
        matches.sort();
        matches.dedup();
        results.append(&mut matches);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn glob_sorts_lexically_and_marks_directories() {
        let tmp = tempfile::tempdir().unwrap();
        for v in ["29.9", "29.10", "30.1"] {
            fs::create_dir_all(tmp.path().join(v).join("sources")).unwrap();
        }
        fs::write(tmp.path().join("31"), "").unwrap();

        // glob sorts lexically by default; "29.10" sorts before "29.9".
        let pat = format!("{}/[0-9]*/sources/", tmp.path().display());
        let got = glob_expand(&pat, Path::new("/nonexistent")).unwrap();
        let expected: Vec<String> = ["29.10", "29.9", "30.1"]
            .iter()
            .map(|v| format!("{}/{v}/sources/", tmp.path().display()))
            .collect();
        assert_eq!(got, expected);

        // Without the trailing slash the regular file matches too.
        let pat = format!("{}/*", tmp.path().display());
        let got = glob_expand(&pat, Path::new("/nonexistent")).unwrap();
        assert_eq!(got.len(), 4);
        assert!(got.iter().all(|p| !p.ends_with('/')));
    }

    #[test]
    fn vsort_orders_version_fragments() {
        let mut env = Environment::new();
        env.add_filter("vsort", |items: Vec<String>| {
            let mut items = items;
            items.sort_by(|a, b| version_cmp(a, b));
            items
        });
        let tmpl = env.render_str(
            "{{ ['v29.9', 'v29.10', 'v30.1'] | vsort | join(',') }}",
            minijinja::context! {},
        );
        assert_eq!(tmpl.unwrap(), "v29.9,v29.10,v30.1");
    }

    #[test]
    fn glob_brace_expansion() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".Brewfile"), "").unwrap();
        fs::write(tmp.path().join("a.txt"), "").unwrap();
        fs::write(tmp.path().join("b.txt"), "").unwrap();
        let dir = tmp.path().display();

        // Only the existing alternative matches.
        let pat = format!("{dir}/{{.Debfile,.Brewfile}}");
        let got = glob_expand(&pat, Path::new("/nonexistent")).unwrap();
        assert_eq!(got, vec![format!("{dir}/.Brewfile")]);

        // Several alternatives each contribute matches.
        let pat = format!("{dir}/{{a,b}}.txt");
        let got = glob_expand(&pat, Path::new("/nonexistent")).unwrap();
        assert_eq!(got, vec![format!("{dir}/a.txt"), format!("{dir}/b.txt")]);

        // A pattern without braces is globbed as-is.
        let pat = format!("{dir}/a.txt");
        let got = glob_expand(&pat, Path::new("/nonexistent")).unwrap();
        assert_eq!(got, vec![format!("{dir}/a.txt")]);
    }

    #[test]
    fn glob_brace_expands_tilde_per_alternative() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join(".zshrc"), "").unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("profile"), "").unwrap();

        // Tilde is expanded inside each brace alternative.
        let pat = format!("{{~/.zshrc,{}/profile}}", other.path().display());
        let got = glob_expand(&pat, home.path()).unwrap();
        assert!(
            got.contains(&format!("{}/.zshrc", home.path().display())),
            "{got:?}"
        );
        assert!(
            got.contains(&format!("{}/profile", other.path().display())),
            "{got:?}"
        );
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn shell_strips_trailing_newlines() {
        assert_eq!(shell_run("/bin/sh", "printf %s hello").unwrap(), "hello");
        assert_eq!(shell_run("/bin/sh", "echo hello").unwrap(), "hello");
        assert_eq!(shell_run("/bin/sh", "printf 'a b\\n\\n'").unwrap(), "a b");
        // Interior newlines are preserved.
        assert_eq!(shell_run("/bin/sh", "printf 'a\\nb\\n'").unwrap(), "a\nb");
    }

    #[test]
    fn shell_accepts_custom_shell() {
        let err = shell_run("", "true").unwrap_err();
        assert!(err.contains("must not be empty"), "{err}");

        let out = shell_run("/bin/sh", "printf %s \"$0\"").unwrap();
        assert_eq!(out, "/bin/sh");
    }

    #[test]
    fn shell_reports_failure() {
        let err = shell_run("/bin/sh", "echo oops >&2; exit 3").unwrap_err();
        assert!(err.contains("exit status: 3"), "{err}");
        assert!(err.contains("with /bin/sh"), "{err}");
        assert!(err.contains("oops"), "{err}");
    }

    #[test]
    fn shell_cache_hit_skips_execution() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("cache");
        let counter = tmp.path().join("counter");
        let cmd = format!("echo x >> {}; echo result", counter.display());
        let ttl = Duration::from_secs(3600);

        assert_eq!(
            shell_run_cached("/bin/sh", &cmd, &cache_dir, ttl).unwrap(),
            "result"
        );
        assert_eq!(
            shell_run_cached("/bin/sh", &cmd, &cache_dir, ttl).unwrap(),
            "result"
        );
        assert_eq!(fs::read_to_string(&counter).unwrap(), "x\n");

        // A zero TTL is always stale and re-runs the command.
        assert_eq!(
            shell_run_cached("/bin/sh", &cmd, &cache_dir, Duration::ZERO).unwrap(),
            "result"
        );
        assert_eq!(fs::read_to_string(&counter).unwrap(), "x\nx\n");
    }

    #[test]
    fn shell_cache_ignores_collisions_and_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("cache");
        let ttl = Duration::from_secs(3600);

        // A cache file recording a different command is a miss.
        let path = cache_dir.join(format!(
            "{:016x}.json",
            fnv1a(&cache_key("/bin/sh", "printf %s new"))
        ));
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(
            &path,
            r#"{"shell":"/bin/sh","command":"other","output":"stale"}"#,
        )
        .unwrap();
        assert_eq!(
            shell_run_cached("/bin/sh", "printf %s new", &cache_dir, ttl).unwrap(),
            "new"
        );

        // Failures are not cached.
        assert!(shell_run_cached("/bin/sh", "exit 1", &cache_dir, ttl).is_err());
        let path = cache_dir.join(format!(
            "{:016x}.json",
            fnv1a(&cache_key("/bin/sh", "exit 1"))
        ));
        assert!(!path.exists());
    }

    #[test]
    fn shell_cache_is_per_shell() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("cache");
        let ttl = Duration::from_secs(3600);

        assert_eq!(
            shell_run_cached("sh", "printf %s \"$0\"", &cache_dir, ttl).unwrap(),
            "sh"
        );
        assert_eq!(
            shell_run_cached("/bin/sh", "printf %s \"$0\"", &cache_dir, ttl).unwrap(),
            "/bin/sh"
        );
        assert_eq!(fs::read_dir(cache_dir).unwrap().count(), 2);
    }
}
