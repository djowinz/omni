import { describe, it, expect } from 'vitest';
import {
  TASK_NAME,
  buildCreateTaskCommand,
  buildDeleteTaskCommand,
  buildQueryTaskCommand,
} from './auto-start-cmd';

describe('auto-start-cmd', () => {
  it('uses a stable task name', () => {
    expect(TASK_NAME).toBe('OmniOverlay');
  });

  describe('buildCreateTaskCommand', () => {
    it('quotes a path containing spaces with backslash-escaped inner quotes (schtasks /tr format)', () => {
      const cmd = buildCreateTaskCommand(String.raw`C:\Program Files\Omni\Omni.exe`);
      expect(cmd).toBe(
        String.raw`schtasks /create /tn "OmniOverlay" /tr "\"C:\Program Files\Omni\Omni.exe\"" /sc ONLOGON /rl HIGHEST /f`,
      );
    });

    it('still wraps a space-free path in escaped quotes for consistency', () => {
      const cmd = buildCreateTaskCommand(String.raw`C:\Omni\Omni.exe`);
      expect(cmd).toBe(
        String.raw`schtasks /create /tn "OmniOverlay" /tr "\"C:\Omni\Omni.exe\"" /sc ONLOGON /rl HIGHEST /f`,
      );
    });

    it('uses ONLOGON schedule and HIGHEST run-level (elevated token for ETW)', () => {
      const cmd = buildCreateTaskCommand(String.raw`C:\Omni\Omni.exe`);
      expect(cmd).toContain('/sc ONLOGON');
      expect(cmd).toContain('/rl HIGHEST');
      expect(cmd).toContain('/f');
    });
  });

  describe('buildDeleteTaskCommand', () => {
    it('targets the same task name with /f to skip the confirmation prompt', () => {
      expect(buildDeleteTaskCommand()).toBe('schtasks /delete /tn "OmniOverlay" /f');
    });
  });

  describe('buildQueryTaskCommand', () => {
    it('queries the same task name', () => {
      expect(buildQueryTaskCommand()).toBe('schtasks /query /tn "OmniOverlay"');
    });
  });
});
