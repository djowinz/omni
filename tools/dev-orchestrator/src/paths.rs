//! Per-platform dev data dir resolution. Pure — pass platform + env in so the
//! functions are fully deterministic in tests.

// Consumers land in later T-tasks (T5 identity mgmt, T6 seed, T7 reset) — the
// whole module is dead code until those land, so silence `dead_code` here.
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::PathBuf;

pub struct PathCtx {
    pub platform: &'static str,
    pub env: fn(&str) -> Option<OsString>,
}

pub fn dev_data_dir(ctx: &PathCtx) -> anyhow::Result<PathBuf> {
    match ctx.platform {
        "windows" => {
            let appdata = (ctx.env)("APPDATA")
                .ok_or_else(|| anyhow::anyhow!("APPDATA env var not set (required on Windows)"))?;
            Ok(PathBuf::from(appdata).join("Omni-dev"))
        }
        _ => {
            if let Some(xdg) = (ctx.env)("XDG_CONFIG_HOME") {
                if !xdg.is_empty() {
                    return Ok(PathBuf::from(xdg).join("Omni-dev"));
                }
            }
            let home = (ctx.env)("HOME").ok_or_else(|| anyhow::anyhow!("HOME env var not set"))?;
            Ok(PathBuf::from(home).join(".config").join("Omni-dev"))
        }
    }
}

pub fn identity_key_path(ctx: &PathCtx) -> anyhow::Result<PathBuf> {
    Ok(dev_data_dir(ctx)?.join("identity.key"))
}

pub fn admin_key_path(ctx: &PathCtx) -> anyhow::Result<PathBuf> {
    Ok(dev_data_dir(ctx)?.join("admin.key"))
}

pub fn logs_dir(ctx: &PathCtx) -> anyhow::Result<PathBuf> {
    Ok(dev_data_dir(ctx)?.join("logs"))
}

/// Default context: reads the running OS's platform string + env.
pub fn default_ctx() -> PathCtx {
    PathCtx {
        platform: std::env::consts::OS, // "windows" | "linux" | "macos"
        env: |k| std::env::var_os(k),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    // `PathCtx::env` is a plain `fn`, not `Fn`, so it can't close over per-test
    // state — the closure reads from a static map. Cargo runs tests in parallel
    // by default, so we serialize with a dedicated `TEST_SERIALIZER` mutex held
    // for each test's full duration; only the test currently holding it is
    // allowed to mutate the data map `ENV_DATA`.
    static TEST_SERIALIZER: Mutex<()> = Mutex::new(());
    static ENV_DATA: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

    struct EnvGuard {
        // Holds `TEST_SERIALIZER` for the test's full duration. Dropped at
        // end-of-test, releasing the next queued test.
        _guard: MutexGuard<'static, ()>,
    }

    fn ctx_with(
        platform: &'static str,
        vars: Vec<(&'static str, &'static str)>,
    ) -> (PathCtx, EnvGuard) {
        // Take the serializer — recover from poisoning (previous test panicked).
        let guard = TEST_SERIALIZER.lock().unwrap_or_else(|p| p.into_inner());
        let data = ENV_DATA.get_or_init(|| Mutex::new(HashMap::new()));
        {
            let mut d = data.lock().unwrap_or_else(|p| p.into_inner());
            d.clear();
            for (k, v) in vars {
                d.insert(k.to_string(), v.to_string());
            }
        }
        let ctx = PathCtx {
            platform,
            env: |k| {
                ENV_DATA
                    .get()
                    .and_then(|m| m.lock().ok())
                    .and_then(|m| m.get(k).map(|s| OsString::from(s.clone())))
            },
        };
        (ctx, EnvGuard { _guard: guard })
    }

    #[test]
    fn dev_data_dir_uses_appdata_on_windows() {
        let (ctx, _g) = ctx_with(
            "windows",
            vec![("APPDATA", "C:\\Users\\X\\AppData\\Roaming")],
        );
        let got = dev_data_dir(&ctx).unwrap();
        assert_eq!(
            got,
            PathBuf::from("C:\\Users\\X\\AppData\\Roaming\\Omni-dev")
        );
    }

    #[test]
    fn dev_data_dir_falls_back_to_home_config_on_linux() {
        let (ctx, _g) = ctx_with("linux", vec![("HOME", "/home/x")]);
        let got = dev_data_dir(&ctx).unwrap();
        assert_eq!(got, PathBuf::from("/home/x/.config/Omni-dev"));
    }

    #[test]
    fn dev_data_dir_honors_xdg_config_home_on_linux() {
        let (ctx, _g) = ctx_with(
            "linux",
            vec![("HOME", "/home/x"), ("XDG_CONFIG_HOME", "/tmp/cfg")],
        );
        let got = dev_data_dir(&ctx).unwrap();
        assert_eq!(got, PathBuf::from("/tmp/cfg/Omni-dev"));
    }

    #[test]
    fn identity_key_path_joins_with_identity_key_filename() {
        let (ctx, _g) = ctx_with("linux", vec![("HOME", "/h")]);
        assert_eq!(
            identity_key_path(&ctx).unwrap(),
            PathBuf::from("/h/.config/Omni-dev/identity.key"),
        );
    }

    #[test]
    fn admin_key_path_joins_with_admin_key_filename() {
        let (ctx, _g) = ctx_with("linux", vec![("HOME", "/h")]);
        assert_eq!(
            admin_key_path(&ctx).unwrap(),
            PathBuf::from("/h/.config/Omni-dev/admin.key"),
        );
    }

    #[test]
    fn logs_dir_joins_with_logs() {
        let (ctx, _g) = ctx_with("linux", vec![("HOME", "/h")]);
        assert_eq!(
            logs_dir(&ctx).unwrap(),
            PathBuf::from("/h/.config/Omni-dev/logs"),
        );
    }

    #[test]
    fn dev_data_dir_errors_without_appdata_on_windows() {
        let (ctx, _g) = ctx_with("windows", vec![]);
        let err = dev_data_dir(&ctx).unwrap_err();
        assert!(err.to_string().contains("APPDATA"));
    }

    #[test]
    fn dev_data_dir_errors_without_home_on_linux() {
        let (ctx, _g) = ctx_with("linux", vec![]);
        let err = dev_data_dir(&ctx).unwrap_err();
        assert!(err.to_string().contains("HOME"));
    }
}
