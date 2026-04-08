import { execSync } from 'child_process';
import { app } from 'electron';
import * as path from 'path';

const TASK_NAME = 'OmniOverlay';

/** Check if the auto-start scheduled task exists. */
export function isAutoStartEnabled(): boolean {
  try {
    execSync(`schtasks /query /tn "${TASK_NAME}"`, { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}

/** Create a scheduled task to run omni-host.exe --service at user logon. */
export function enableAutoStart(): boolean {
  const hostPath = path.join(path.dirname(app.getPath('exe')), 'omni-host.exe');
  try {
    execSync(
      `schtasks /create /tn "${TASK_NAME}" /tr "\\"${hostPath}\\" --service" /sc ONLOGON /rl LIMITED /f`,
      { stdio: 'pipe' },
    );
    return true;
  } catch {
    return false;
  }
}

/** Remove the auto-start scheduled task. */
export function disableAutoStart(): boolean {
  try {
    execSync(`schtasks /delete /tn "${TASK_NAME}" /f`, { stdio: 'pipe' });
    return true;
  } catch {
    return false;
  }
}
