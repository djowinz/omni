/// Process scanner: enumerates running processes and spawns the external
/// overlay for any new process with at least one visible window.
use std::collections::{HashMap, HashSet};
use std::process::Child;

use tracing::{error, info, warn};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
};

use crate::config::Config;
use crate::win32;

/// Why an overlay was spawned for a given process. Used by `set_config` to
/// decide whether a tracked overlay should be torn down after the user edits
/// `include` / `exclude` / `game_directories`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpawnReason {
    /// Process exe matched the user's include list (always-on early-exit).
    Include,
    /// Process path matched a configured game directory prefix (heuristic).
    GameDirectory,
}

struct TrackedOverlay {
    child: Child,
    reason: SpawnReason,
}

pub struct Scanner {
    seen: HashSet<u32>,
    tracked: HashMap<u32, TrackedOverlay>,
    /// Pids confirmed in a configured game directory. Includes pids whose own
    /// path query matched, plus pids that inherited the verdict from an
    /// ancestor (UE-style launcher → shipping process where anti-cheat blocks
    /// `OpenProcess` on the child).
    game_dir_pids: HashSet<u32>,
    /// Pids we've already logged a "no visible window yet" message for. Stops
    /// log spam while a launcher process waits silently for its child.
    notified_no_window: HashSet<u32>,
    overlay_exe_path: String,
    config: Config,
    last_game_exe: Option<String>,
    last_external_pid: Option<u32>,
    last_game_hwnd: Option<isize>,
    last_process_count: usize,
    last_seen_count: usize,
}

impl Scanner {
    pub fn new(overlay_exe_path: String, config: Config) -> Self {
        Self {
            seen: HashSet::new(),
            tracked: HashMap::new(),
            game_dir_pids: HashSet::new(),
            notified_no_window: HashSet::new(),
            overlay_exe_path,
            config,
            last_process_count: 0,
            last_seen_count: 0,
            last_game_exe: None,
            last_external_pid: None,
            last_game_hwnd: None,
        }
    }

    pub fn last_game_exe(&self) -> Option<&str> {
        self.last_game_exe.as_deref()
    }

    pub fn last_external_pid(&self) -> Option<u32> {
        self.last_external_pid
    }

    pub fn last_game_hwnd(&self) -> Option<isize> {
        self.last_game_hwnd
    }

    /// Replace the scanner's config and re-evaluate state so include / exclude /
    /// game_directories edits take effect immediately, not just for processes
    /// that start after the edit.
    ///
    /// Tear-down rules (per `tear_down_reason`):
    /// - Include is the always-on early-exit; never tear down include-matched processes.
    /// - Exclude is the always-off early-exit; tear down regardless of spawn reason.
    /// - Game-directory is heuristic; tear down when the spawn reason was a
    ///   game-directory match and the process path no longer matches any
    ///   configured directory prefix.
    ///
    /// Then clears the `seen` cache (preserving entries for surviving tracked
    /// pids so we don't double-spawn) so previously-skipped processes get
    /// re-walked against the new rules on the next poll.
    pub fn set_config(&mut self, config: Config) {
        let prev_tracked = self.tracked.len();

        // Build a pid → (exe_name, exe_path) map so we can evaluate tear-down
        // rules without re-opening processes per tracked pid.
        let processes = win32::iter_processes().unwrap_or_default();
        let exe_by_pid: HashMap<u32, String> = processes
            .iter()
            .map(|e| (e.th32ProcessID, win32::wchar_to_string(&e.szExeFile)))
            .collect();

        let to_kill: Vec<(u32, &'static str)> = self
            .tracked
            .iter()
            .filter_map(|(&pid, overlay)| {
                let exe_name = exe_by_pid.get(&pid)?;
                let exe_path = win32::get_process_exe_path(pid).ok();
                tear_down_reason(overlay.reason, exe_name, exe_path.as_deref(), &config)
                    .map(|reason| (pid, reason))
            })
            .collect();

        for (pid, reason) in &to_kill {
            if let Some(mut overlay) = self.tracked.remove(pid) {
                info!(
                    pid = *pid,
                    reason = *reason,
                    "Killing external overlay — config update changed eligibility"
                );
                if let Err(e) = overlay.child.kill() {
                    warn!(pid = *pid, error = %e, "Failed to kill external overlay on config update");
                }
            }
        }

        if self.tracked.is_empty() && prev_tracked > 0 {
            self.last_game_exe = None;
            self.last_game_hwnd = None;
            self.last_external_pid = None;
        }

        self.config = config;

        self.seen.clear();
        self.seen.extend(self.tracked.keys().copied());
    }

    pub fn poll(&mut self) {
        let processes = match win32::iter_processes() {
            Ok(p) => p,
            Err(e) => {
                error!(error = %e, "Failed to enumerate processes");
                return;
            }
        };

        let alive: HashSet<u32> = processes.iter().map(|e| e.th32ProcessID).collect();
        let process_count = alive.len();

        self.seen.retain(|pid| alive.contains(pid));
        self.game_dir_pids.retain(|pid| alive.contains(pid));
        self.notified_no_window.retain(|pid| alive.contains(pid));
        let seen_count = self.seen.len();

        // Parent-pid map straight from the toolhelp snapshot — no `OpenProcess`
        // required, so this works even for anti-cheat-protected processes.
        let parent_by_pid: HashMap<u32, u32> = processes
            .iter()
            .map(|e| (e.th32ProcessID, e.th32ParentProcessID))
            .collect();

        if process_count != self.last_process_count || seen_count != self.last_seen_count {
            info!(
                process_count,
                seen = seen_count,
                tracked = self.tracked.len(),
                "Scanner poll"
            );
            self.last_process_count = process_count;
            self.last_seen_count = seen_count;
        }
        let prev_tracked_count = self.tracked.len();
        self.tracked.retain(|pid, overlay| {
            if alive.contains(pid) {
                true
            } else {
                if let Err(e) = overlay.child.kill() {
                    warn!(error = %e, "Failed to kill external overlay process");
                }
                false
            }
        });
        // If all tracked games exited, clear stale game state
        if self.tracked.is_empty() && prev_tracked_count > 0 {
            self.last_game_exe = None;
            self.last_game_hwnd = None;
            self.last_external_pid = None;
        }

        for entry in &processes {
            let pid = entry.th32ProcessID;

            if pid <= 4 {
                continue;
            }
            if self.seen.contains(&pid) {
                continue;
            }

            let exe_name = win32::wchar_to_string(&entry.szExeFile);

            // Always skip our own processes and the Electron app
            const SELF_EXECUTABLES: &[&str] = &[
                "omni-host.exe",
                "omni-overlay.exe",
                "omni.exe",     // Installed Electron app
                "electron.exe", // Dev mode Electron
                "nextron.exe",  // Dev mode Nextron
            ];
            if SELF_EXECUTABLES
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&exe_name))
            {
                self.seen.insert(pid);
                continue;
            }

            // Explicit include list — always spawn overlay, skip all other checks
            let in_include = self
                .config
                .include
                .iter()
                .any(|inc| inc.eq_ignore_ascii_case(&exe_name));
            if in_include {
                if let Some(game_hwnd) = find_visible_window(pid) {
                    info!(pid, exe_name, "Spawning overlay (include list)");
                    self.last_game_hwnd = Some(game_hwnd.0 as isize);
                    self.spawn_external_overlay(pid, &exe_name, game_hwnd, SpawnReason::Include);
                    self.seen.insert(pid); // Only mark seen after successful spawn
                } else {
                    info!(
                        pid,
                        exe_name, "Pending overlay — no visible window yet (include list)"
                    );
                }
                continue;
            }

            let excluded = self
                .config
                .exclude
                .iter()
                .any(|ex| ex.eq_ignore_ascii_case(&exe_name));
            if excluded {
                self.seen.insert(pid);
                continue;
            }

            // Skip common helper/launcher patterns that live inside game directories
            // but aren't games themselves
            let exe_lower = exe_name.to_lowercase();
            let is_helper = exe_lower.contains("helper")
                || exe_lower.contains("launcher")
                || exe_lower.contains("crashhandler")
                || exe_lower.contains("crashreport")
                || exe_lower.contains("crashpad")
                || exe_lower.contains("webhelper")
                || exe_lower.contains("prereq")
                || exe_lower.contains("installer")
                || exe_lower.contains("setup")
                || exe_lower.contains("updater")
                || exe_lower.contains("unins");
            if is_helper {
                self.seen.insert(pid);
                continue;
            }

            // Resolve the game-directory verdict. If we can read our own path,
            // check it directly. If `OpenProcess` is denied (commonly because
            // an anti-cheat protected the process — typical for the actual
            // game exe in launcher → shipping setups like Unreal), inherit the
            // verdict from any ancestor we've already verified, so the child
            // gets the overlay even though we can't introspect it.
            match win32::get_process_exe_path(pid) {
                Ok(path) => {
                    let path_lower = path.to_lowercase();
                    let matched = self
                        .config
                        .game_directories
                        .iter()
                        .any(|dir| path_lower.contains(&dir.to_lowercase()));
                    if matched {
                        self.game_dir_pids.insert(pid);
                    } else {
                        // Definitively not a game — add to seen, never retry
                        self.seen.insert(pid);
                        continue;
                    }
                }
                Err(_) => {
                    let inherited = walk_ancestors(&parent_by_pid, pid)
                        .iter()
                        .any(|a| self.game_dir_pids.contains(a));
                    if inherited {
                        // Cache the verdict so descendants of this process can
                        // also inherit even if its launcher exits.
                        self.game_dir_pids.insert(pid);
                    } else {
                        // Can't verify and no known ancestor — silently retry.
                        // (No `seen.insert`: a future poll may reveal an ancestor
                        // that *does* match, e.g. once the launcher has been
                        // walked.)
                        continue;
                    }
                }
            }

            // Require a visible window — retry if not yet visible.
            let game_hwnd = match find_visible_window(pid) {
                Some(h) => h,
                None => {
                    if self.notified_no_window.insert(pid) {
                        info!(
                            pid,
                            exe_name,
                            "Pending overlay — no visible window yet (game directory)"
                        );
                    }
                    continue;
                }
            };

            info!(pid, exe_name, "Spawning overlay");
            self.last_game_hwnd = Some(game_hwnd.0 as isize);
            self.spawn_external_overlay(pid, &exe_name, game_hwnd, SpawnReason::GameDirectory);
            self.seen.insert(pid);

            // Retire any ancestors we've already classified as game-dir matches:
            // a launcher whose visible window we were waiting on (or whose own
            // overlay we attached earlier) hands off to the descendant that
            // actually owns the game window. Mark the launcher seen so we stop
            // re-evaluating it, and kill its overlay if one was attached.
            if self.tracked.contains_key(&pid) {
                for ancestor_pid in walk_ancestors(&parent_by_pid, pid) {
                    if !self.game_dir_pids.contains(&ancestor_pid) {
                        continue;
                    }
                    if self.seen.insert(ancestor_pid) {
                        info!(
                            launcher_pid = ancestor_pid,
                            descendant_pid = pid,
                            "Retiring launcher — overlay attached to descendant"
                        );
                    }
                    if let Some(mut overlay) = self.tracked.remove(&ancestor_pid) {
                        info!(
                            launcher_pid = ancestor_pid,
                            "Killing launcher overlay — descendant has visible window"
                        );
                        if let Err(e) = overlay.child.kill() {
                            warn!(
                                launcher_pid = ancestor_pid,
                                error = %e,
                                "Failed to kill launcher overlay"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Kill all tracked external overlay child processes.
    pub fn kill_all(&mut self) {
        for (pid, mut overlay) in self.tracked.drain() {
            info!(pid, "Killing external overlay process");
            if let Err(e) = overlay.child.kill() {
                warn!(error = %e, "Failed to kill external overlay process");
            }
        }
    }

    /// Iterate over tracked game PIDs.
    pub fn tracked_pids(&self) -> impl Iterator<Item = &u32> {
        self.tracked.keys()
    }

    /// Check if a PID is currently tracked.
    pub fn is_tracked(&self, pid: u32) -> bool {
        self.tracked.contains_key(&pid)
    }

    fn spawn_external_overlay(
        &mut self,
        pid: u32,
        exe_name: &str,
        hwnd: HWND,
        reason: SpawnReason,
    ) {
        let hwnd_value = hwnd.0 as isize;
        info!(
            pid,
            exe_name,
            hwnd = hwnd_value,
            "Spawning external overlay process"
        );

        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        match std::process::Command::new(&self.overlay_exe_path)
            .args(["--hwnd", &hwnd_value.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
        {
            Ok(child) => {
                self.tracked.insert(pid, TrackedOverlay { child, reason });
                self.last_game_exe = Some(exe_name.to_string());
                self.last_external_pid = Some(pid);
            }
            Err(e) => {
                warn!(pid, exe_name, error = %e, "Failed to spawn external overlay process");
            }
        }
    }
}

/// Decide whether a tracked overlay should be torn down after a config update.
/// Returns `Some(reason_label)` for logging, or `None` to keep the overlay.
///
/// Order matches the user-facing semantic: include is the always-on early-exit
/// (wins over exclude and game-directory-mismatch); exclude is always-off
/// (wins over game-directory match); game-directory mismatch only tears down
/// overlays that were originally spawned via a game-directory match.
fn tear_down_reason(
    reason: SpawnReason,
    exe_name: &str,
    exe_path: Option<&str>,
    config: &Config,
) -> Option<&'static str> {
    if config
        .include
        .iter()
        .any(|inc| inc.eq_ignore_ascii_case(exe_name))
    {
        return None;
    }
    if config
        .exclude
        .iter()
        .any(|ex| ex.eq_ignore_ascii_case(exe_name))
    {
        return Some("now in exclude list");
    }
    if reason == SpawnReason::GameDirectory {
        let path_lower = exe_path.unwrap_or("").to_lowercase();
        // If we couldn't query the path, conservatively keep the overlay.
        if path_lower.is_empty() {
            return None;
        }
        let still_matches = config
            .game_directories
            .iter()
            .any(|dir| path_lower.contains(&dir.to_lowercase()));
        if !still_matches {
            return Some("game directory removed");
        }
    }
    None
}

// Scanner-specific helpers (not shared)

/// Walk a process's ancestor chain via the toolhelp parent map. Returns the
/// ancestor pids in order (immediate parent first). Bounded depth so a stale
/// snapshot or a self-referential entry can't loop forever; 8 levels is well
/// past any realistic process tree.
fn walk_ancestors(parent_by_pid: &HashMap<u32, u32>, pid: u32) -> Vec<u32> {
    const MAX_DEPTH: usize = 8;
    let mut chain = Vec::new();
    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        let parent = match parent_by_pid.get(&current).copied() {
            Some(p) if p != 0 && p != current => p,
            _ => break,
        };
        chain.push(parent);
        current = parent;
    }
    chain
}

fn find_visible_window(pid: u32) -> Option<HWND> {
    struct CallbackData {
        target_pid: u32,
        found_hwnd: Option<HWND>,
    }

    // SAFETY: This callback is invoked synchronously by EnumWindows below.
    // `lparam` points to a valid `CallbackData` on the stack, which outlives
    // the EnumWindows call.
    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut CallbackData);
        let mut window_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
        if window_pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
            data.found_hwnd = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut data = CallbackData {
        target_pid: pid,
        found_hwnd: None,
    };
    let _ = unsafe { EnumWindows(Some(enum_cb), LPARAM(&mut data as *mut _ as isize)) };
    data.found_hwnd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn scanner_new_starts_empty() {
        let scanner = Scanner::new("overlay.exe".to_string(), Config::default());
        assert!(scanner.tracked.is_empty());
        assert!(scanner.seen.is_empty());
    }

    #[test]
    fn last_game_exe_starts_none() {
        let scanner = Scanner::new("overlay.exe".to_string(), Config::default());
        assert_eq!(scanner.last_game_exe(), None);
    }

    #[test]
    fn set_config_clears_seen_so_new_rules_apply() {
        let mut scanner = Scanner::new("overlay.exe".to_string(), Config::default());
        scanner.seen.insert(1234);
        scanner.seen.insert(5678);

        let mut new_config = Config::default();
        new_config.exclude.push("foo.exe".to_string());
        new_config.include.push("bar.exe".to_string());
        scanner.set_config(new_config);

        assert!(
            scanner.seen.is_empty(),
            "seen should be cleared (no surviving tracked pids) so previously-skipped pids get re-walked under the new rules"
        );
        assert!(scanner.config.exclude.iter().any(|e| e == "foo.exe"));
        assert!(scanner.config.include.iter().any(|i| i == "bar.exe"));
    }

    fn cfg(include: &[&str], exclude: &[&str], dirs: &[&str]) -> Config {
        Config {
            include: include.iter().map(|s| s.to_string()).collect(),
            exclude: exclude.iter().map(|s| s.to_string()).collect(),
            game_directories: dirs.iter().map(|s| s.to_string()).collect(),
            ..Config::default()
        }
    }

    #[test]
    fn tear_down_include_overrides_exclude_and_game_dir_removal() {
        // Process is in include AND in exclude AND its directory is no longer
        // configured. Include is always-on, so we must NOT tear down.
        let config = cfg(&["foo.exe"], &["foo.exe"], &[]);
        assert_eq!(
            tear_down_reason(
                SpawnReason::GameDirectory,
                "foo.exe",
                Some(r"C:\Games\Foo\foo.exe"),
                &config,
            ),
            None,
        );
        assert_eq!(
            tear_down_reason(SpawnReason::Include, "foo.exe", None, &config),
            None,
        );
    }

    #[test]
    fn tear_down_exclude_kills_regardless_of_spawn_reason() {
        let config = cfg(&[], &["foo.exe"], &[r"games\"]);
        assert_eq!(
            tear_down_reason(
                SpawnReason::GameDirectory,
                "foo.exe",
                Some(r"C:\Games\Foo\foo.exe"),
                &config,
            ),
            Some("now in exclude list"),
        );
        assert_eq!(
            tear_down_reason(
                SpawnReason::Include,
                "foo.exe",
                Some(r"C:\Games\Foo\foo.exe"),
                &config,
            ),
            Some("now in exclude list"),
        );
    }

    #[test]
    fn tear_down_game_dir_removed_kills_only_game_dir_spawn() {
        // Path no longer matches any configured game directory.
        let config = cfg(&[], &[], &[r"other\"]);
        assert_eq!(
            tear_down_reason(
                SpawnReason::GameDirectory,
                "foo.exe",
                Some(r"C:\Games\Foo\foo.exe"),
                &config,
            ),
            Some("game directory removed"),
        );
        // Include-spawned overlays are immune to game-directory changes.
        assert_eq!(
            tear_down_reason(
                SpawnReason::Include,
                "foo.exe",
                Some(r"C:\Games\Foo\foo.exe"),
                &config,
            ),
            None,
        );
    }

    #[test]
    fn tear_down_keeps_overlay_when_game_dir_still_matches() {
        let config = cfg(&[], &[], &[r"games\"]);
        assert_eq!(
            tear_down_reason(
                SpawnReason::GameDirectory,
                "foo.exe",
                Some(r"C:\Games\Foo\foo.exe"),
                &config,
            ),
            None,
        );
    }

    #[test]
    fn tear_down_keeps_overlay_when_path_unknown() {
        // If we can't query the process path, conservatively keep the overlay
        // rather than killing on uncertainty.
        let config = cfg(&[], &[], &[r"other\"]);
        assert_eq!(
            tear_down_reason(SpawnReason::GameDirectory, "foo.exe", None, &config),
            None,
        );
    }

    fn parent_map(pairs: &[(u32, u32)]) -> HashMap<u32, u32> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn walk_ancestors_returns_chain_root_last() {
        // pid 30's parent is 20, 20's parent is 10, 10's parent is 0 (root).
        let map = parent_map(&[(30, 20), (20, 10), (10, 0)]);
        assert_eq!(walk_ancestors(&map, 30), vec![20, 10]);
    }

    #[test]
    fn walk_ancestors_stops_on_unknown_parent() {
        let map = parent_map(&[(30, 20)]); // 20 has no entry
        assert_eq!(walk_ancestors(&map, 30), vec![20]);
    }

    #[test]
    fn walk_ancestors_breaks_on_self_cycle() {
        let map = parent_map(&[(30, 30)]); // self-loop guard
        assert!(walk_ancestors(&map, 30).is_empty());
    }

    #[test]
    fn walk_ancestors_bounded_depth_on_cycle() {
        // a → b → a; depth limit kicks in.
        let map = parent_map(&[(1, 2), (2, 1)]);
        let chain = walk_ancestors(&map, 1);
        assert!(chain.len() <= 8);
        assert_eq!(chain[0], 2);
    }

    #[test]
    fn walk_ancestors_empty_when_pid_missing() {
        let map = parent_map(&[]);
        assert!(walk_ancestors(&map, 42).is_empty());
    }
}
