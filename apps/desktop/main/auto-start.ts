import { execSync } from 'child_process';
import { app } from 'electron';
import {
  TASK_NAME,
  buildCreateTaskCommand,
  buildDeleteTaskCommand,
  buildQueryTaskCommand,
} from './auto-start-cmd';

/// True if the OmniOverlay scheduled task exists in the current user's task
/// scheduler.
export function isAutoStartEnabled(): boolean {
  try {
    execSync(buildQueryTaskCommand(), { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

/// Register the Electron app to launch at user logon via Task Scheduler.
///
/// The Electron main process auto-spawns `omni-host` (HostManager.start()) and
/// honors `minimize_to_tray`, so this single registration boots the tray icon,
/// the UI window (or hidden if minimized-to-tray), and the overlay backend.
export function enableAutoStart(): boolean {
  // Clear any legacy `HKCU\…\Run\<productName>` registration left by the
  // previous Electron-API-based implementation. Without this, users who had
  // the old toggle enabled would end up with both startup hooks firing — the
  // Run-key entry typically pointing at a stale path.
  clearLegacyRunKey();

  const exePath = app.getPath('exe');
  try {
    execSync(buildCreateTaskCommand(exePath), { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

/// Remove both the scheduled task and any legacy `Run`-key entry, so disabling
/// the toggle is comprehensive regardless of which mechanism originally
/// registered the app.
export function disableAutoStart(): boolean {
  clearLegacyRunKey();
  try {
    execSync(buildDeleteTaskCommand(), { stdio: 'pipe' });
    return true;
  } catch {
    // Task may not exist (already disabled, or never enabled). Treat as
    // success — the user-visible state is correct: the app does not run at
    // logon.
    return true;
  }
}

function clearLegacyRunKey(): void {
  try {
    app.setLoginItemSettings({ openAtLogin: false });
  } catch {
    /* best effort */
  }
}

export { TASK_NAME };
