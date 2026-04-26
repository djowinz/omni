/// Pure command-string builders for the Windows Task Scheduler `schtasks`
/// registration that backs the "Start with Windows" toggle. Split out from
/// `auto-start.ts` so unit tests can run without loading the Electron module.
///
/// Why Task Scheduler instead of Electron's `app.setLoginItemSettings`:
/// `setLoginItemSettings` writes `HKCU\…\Run\<productName>` pointing at
/// `process.execPath`. In dev that's `node_modules\electron\dist\electron.exe`,
/// and after Squirrel/electron-builder updates the recorded path can be stale,
/// so the toggle reads back as enabled but Windows runs nothing on logon.

export const TASK_NAME = 'OmniOverlay';

/// Build the `schtasks /create …` invocation that registers `exePath` to run
/// at user logon.
///
/// Path quoting: `schtasks /tr` expects `"\"path with spaces\" args"`. We wrap
/// the whole `/tr` argument in double quotes and escape the inner quotes with
/// backslashes, which is the form Microsoft documents for paths containing
/// spaces. `cmd.exe` treats backslash-quote pairs as a literal quote inside an
/// outer quoted region, so the path lands at `schtasks` correctly.
///
/// Run level is `HIGHEST` so the task launches with the user's elevated token
/// when they're in the Administrators group — this is the scheduled-task
/// elevation pattern that runs without a UAC prompt at logon, and is required
/// for ETW providers (the overlay backend's frame-presentation tracing) to be
/// accessible. Non-admin users degrade gracefully to their normal token.
export function buildCreateTaskCommand(exePath: string): string {
  return `schtasks /create /tn "${TASK_NAME}" /tr "\\"${exePath}\\"" /sc ONLOGON /rl HIGHEST /f`;
}

export function buildDeleteTaskCommand(): string {
  return `schtasks /delete /tn "${TASK_NAME}" /f`;
}

export function buildQueryTaskCommand(): string {
  return `schtasks /query /tn "${TASK_NAME}"`;
}
