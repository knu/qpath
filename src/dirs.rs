use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Base directories used as built-in template variables.
pub struct BaseDirs {
    pub home: PathBuf,
    pub config_home: PathBuf,
    pub data_home: PathBuf,
    pub cache_home: PathBuf,
    pub state_home: PathBuf,
}

impl BaseDirs {
    pub fn from_env() -> Result<Self> {
        let home = env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(env::home_dir)
            .context("cannot determine home directory")?;
        Ok(Self::new(home, cfg!(target_os = "macos")))
    }

    fn new(home: PathBuf, macos: bool) -> Self {
        let xdg = |var: &str, default: &str| {
            env::var_os(var)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(default))
        };
        let (config_home, data_home, cache_home, state_home) = if macos {
            let app_support = home.join("Library/Application Support");
            (
                app_support.clone(),
                app_support.clone(),
                home.join("Library/Caches"),
                app_support,
            )
        } else {
            (
                xdg("XDG_CONFIG_HOME", ".config"),
                xdg("XDG_DATA_HOME", ".local/share"),
                xdg("XDG_CACHE_HOME", ".cache"),
                xdg("XDG_STATE_HOME", ".local/state"),
            )
        };
        BaseDirs {
            home,
            config_home,
            data_home,
            cache_home,
            state_home,
        }
    }

    /// Directory holding qpath's own definition files, XDG-style on all
    /// platforms.
    pub fn qpath_config_dir(&self) -> PathBuf {
        env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(".config"))
            .join("qpath")
    }

    /// Directory holding qpath's own cache files, XDG-style on all platforms.
    pub fn qpath_cache_dir(&self) -> PathBuf {
        env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(".cache"))
            .join("qpath")
    }
}

/// Expand a leading `~`, `~/`, or `~user` prefix.  `~user` is resolved with
/// `getpwnam`; an unknown user leaves the path as written.  Shortening back
/// to `~user` form is intentionally not supported.
pub fn expand_tilde(path: &str, home: &Path) -> String {
    if path == "~" {
        home.display().to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{}", home.display(), rest)
    } else if let Some(rest) = path.strip_prefix('~') {
        let (user, tail) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        match user_home(user) {
            Some(user_home) => format!("{}{tail}", user_home.display()),
            None => path.to_string(),
        }
    } else {
        path.to_string()
    }
}

/// Look up a user's home directory with `getpwnam_r`.
#[cfg(unix)]
fn user_home(user: &str) -> Option<PathBuf> {
    use std::ffi::{CStr, CString, OsString};
    use std::os::unix::ffi::OsStringExt;

    let name = CString::new(user).ok()?;
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 1024];
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    loop {
        let ret = unsafe {
            libc::getpwnam_r(
                name.as_ptr(),
                &mut pwd,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        if ret == libc::ERANGE && buf.len() < 1 << 20 {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if ret != 0 || result.is_null() {
            return None;
        }
        let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
        return Some(PathBuf::from(OsString::from_vec(dir.to_bytes().to_vec())));
    }
}

#[cfg(not(unix))]
fn user_home(_user: &str) -> Option<PathBuf> {
    None
}

pub fn shorten_home(path: &str, home: &Path) -> String {
    let home = home.display().to_string();
    if path == home {
        return "~".to_string();
    }
    match path.strip_prefix(&home).and_then(|s| s.strip_prefix('/')) {
        Some(rest) => format!("~/{rest}"),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_expansion() {
        let home = Path::new("/home/u");
        assert_eq!(expand_tilde("~", home), "/home/u");
        assert_eq!(expand_tilde("~/a/b/", home), "/home/u/a/b/");
        assert_eq!(expand_tilde("/etc/hosts", home), "/etc/hosts");
    }

    #[test]
    #[cfg(unix)]
    fn tilde_user_expansion() {
        let home = Path::new("/home/u");
        let root_home = user_home("root").unwrap().display().to_string();
        assert_eq!(expand_tilde("~root", home), root_home);
        assert_eq!(
            expand_tilde("~root/x y/", home),
            format!("{root_home}/x y/")
        );
        // Unknown users leave the path as written.
        assert_eq!(expand_tilde("~no-such-user/x", home), "~no-such-user/x");
    }

    #[test]
    fn home_shortening() {
        let home = Path::new("/home/u");
        assert_eq!(shorten_home("/home/u", home), "~");
        assert_eq!(shorten_home("/home/u/a/b/", home), "~/a/b/");
        assert_eq!(shorten_home("/home/uu/a", home), "/home/uu/a");
        assert_eq!(shorten_home("/etc/hosts", home), "/etc/hosts");
    }
}
