import { useMemo, useState } from 'react';
import { ShieldCheck } from 'lucide-react';

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  PasswordStrengthMeter,
  computeStrength,
  type PasswordStrength,
} from '@/components/ui/password-strength-meter';
import {
  mapErrorToUserMessage,
  type OmniError,
  type UserFacingError,
} from '@/lib/map-error-to-user-message';
import { useBackend } from '@/hooks/use-backend';

export type IdentityBackupMode = 'first-publish' | 'settings' | 'forced-rotation';

export interface IdentityBackupDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess: (backupPath: string) => void;
  mode: IdentityBackupMode;
  /**
   * Pluggable persistence step. Receives the decoded encrypted bytes from
   * `identity.backup` and must return the final filesystem path. Tests and the
   * smoke page pass a mock; in real Electron use, the default below invokes
   * the main-process `dialog:saveIdentityBackup` + `fs:writeFile` IPC
   * handlers — note that those handlers are NOT yet wired (#016 lands them).
   */
  saveBackup?: (encryptedBytes: Uint8Array) => Promise<string>;
}

const MIN_PASSPHRASE_LENGTH = 12;
const STRENGTH_RANK: Record<PasswordStrength, number> = {
  none: 0,
  weak: 1,
  medium: 2,
  strong: 3,
};

const MODE_COPY: Record<IdentityBackupMode, { title: string; description: string }> = {
  'first-publish': {
    title: 'Back up your identity',
    description:
      'Before publishing for the first time, save an encrypted backup of your signing key. Without it you cannot recover your author identity if this device is lost.',
  },
  settings: {
    title: 'Back up your identity',
    description:
      'Save an encrypted backup of your signing key. Store the file somewhere safe — without it you cannot recover your author identity.',
  },
  'forced-rotation': {
    title: 'Back up before rotating',
    description:
      'Your signing key is being rotated. Save an encrypted backup of the current key before continuing.',
  },
};

function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    out[i] = binary.charCodeAt(i);
  }
  return out;
}

function isOmniError(value: unknown): value is OmniError {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as Record<string, unknown>;
  return typeof v.code === 'string' && typeof v.kind === 'string' && typeof v.message === 'string';
}

function toUserFacing(err: unknown): UserFacingError {
  if (isOmniError(err)) {
    return mapErrorToUserMessage(err);
  }
  const message = err instanceof Error && err.message ? err.message : 'Save failed';
  return mapErrorToUserMessage({
    code: 'HOST_LOCAL',
    kind: 'HostLocal',
    message,
  });
}

/**
 * Default persistence implementation. References main-process IPC handlers
 * that do NOT exist yet (`dialog:saveIdentityBackup`, `fs:writeFile`) — those
 * are scoped to #016. Calling this in the current Electron build will throw.
 * Tests + the smoke page must pass a `saveBackup` prop instead.
 */
async function defaultSaveBackup(bytes: Uint8Array): Promise<string> {
  const electron = (window as any).electron;
  if (!electron?.ipcRenderer?.invoke) {
    throw new Error('Save bridge not available');
  }
  const path: string | undefined = await electron.ipcRenderer.invoke('dialog:saveIdentityBackup');
  if (!path) throw new Error('Save cancelled');
  await electron.ipcRenderer.invoke('fs:writeFile', { path, bytes });
  return path;
}

export function IdentityBackupDialog({
  open,
  onOpenChange,
  onSuccess,
  mode,
  saveBackup,
}: IdentityBackupDialogProps) {
  const backend = useBackend();
  const [passphrase, setPassphrase] = useState('');
  const [confirm, setConfirm] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<UserFacingError | null>(null);
  const [copyStatus, setCopyStatus] = useState<'idle' | 'copied'>('idle');

  const strength = useMemo(() => computeStrength(passphrase, MIN_PASSPHRASE_LENGTH), [passphrase]);
  const passphrasesMatch = passphrase.length > 0 && passphrase === confirm;
  const meetsLength = passphrase.length >= MIN_PASSPHRASE_LENGTH;
  const meetsStrength = STRENGTH_RANK[strength] >= STRENGTH_RANK.medium;
  const canSubmit = meetsLength && passphrasesMatch && meetsStrength && !submitting;

  const copy = MODE_COPY[mode];

  const reset = () => {
    setPassphrase('');
    setConfirm('');
    setError(null);
    setCopyStatus('idle');
  };

  const handleOpenChange = (next: boolean) => {
    if (!next) reset();
    onOpenChange(next);
  };

  const handleSubmit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setError(null);
    setCopyStatus('idle');
    try {
      // identity.backup is routed through the share:ws-message IPC channel,
      // not the generic ws-message bridge — the host dispatches it from
      // `share::ws_messages.rs`. Per the wire-shape rule (feedback memory)
      // params are nested under "params", and the response envelope is
      // { id, type: 'identity.backupResult' | 'error', params | error }.
      const bridge = window.omni?.sendShareMessage;
      if (!bridge) throw new Error('IPC share bridge not available');
      void backend; // reserved for future BackendApi migration
      const id = crypto.randomUUID();
      const response = (await bridge({
        id,
        type: 'identity.backup',
        params: { passphrase },
      })) as {
        id?: string;
        type?: string;
        params?: { encrypted_bytes_b64?: unknown };
        error?: { message?: string; detail?: string };
      };
      if (response.type === 'error') {
        throw new Error(response.error?.message ?? response.error?.detail ?? 'identity.backup failed');
      }
      const b64 = response.params?.encrypted_bytes_b64;
      if (typeof b64 !== 'string') {
        throw new Error('Malformed identity.backup response');
      }
      const bytes = base64ToBytes(b64);
      const path = await (saveBackup ?? defaultSaveBackup)(bytes);
      onSuccess(path);
      reset();
      onOpenChange(false);
    } catch (err) {
      const mapped = toUserFacing(err);

      console.error('[identity-backup-dialog] save failed', mapped.opaquePayload);
      setError(mapped);
    } finally {
      setSubmitting(false);
    }
  };

  const handleReport = async () => {
    if (!error) return;
    try {
      await navigator.clipboard.writeText(error.opaquePayload);
      setCopyStatus('copied');
    } catch {
      // Clipboard can fail in non-secure contexts; surface nothing extra.
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="h-5 w-5" />
            {copy.title}
          </DialogTitle>
          <DialogDescription>{copy.description}</DialogDescription>
        </DialogHeader>

        <form
          className="flex flex-col gap-4 py-2"
          onSubmit={(e) => {
            e.preventDefault();
            handleSubmit();
          }}
        >
          <div className="flex flex-col gap-2">
            <Label htmlFor="identity-backup-passphrase">Passphrase</Label>
            <Input
              id="identity-backup-passphrase"
              type="password"
              autoComplete="new-password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              disabled={submitting}
              autoFocus
            />
            <PasswordStrengthMeter value={passphrase} minLength={MIN_PASSPHRASE_LENGTH} />
            <p className="text-xs text-muted-foreground">
              At least {MIN_PASSPHRASE_LENGTH} characters. Mix cases, digits, and symbols to reach
              &ldquo;Medium&rdquo; or stronger.
            </p>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="identity-backup-confirm">Confirm passphrase</Label>
            <Input
              id="identity-backup-confirm"
              type="password"
              autoComplete="new-password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              disabled={submitting}
            />
            {confirm.length > 0 && !passphrasesMatch && (
              <p className="text-xs text-destructive">Passphrases do not match.</p>
            )}
          </div>

          {error && (
            <div
              role="alert"
              className="flex flex-col gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
            >
              <span>{error.text}</span>
              <div className="flex items-center gap-2">
                <Button type="button" variant="outline" size="sm" onClick={handleReport}>
                  {copyStatus === 'copied' ? 'Copied' : 'Report this'}
                </Button>
              </div>
            </div>
          )}

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => handleOpenChange(false)}
              disabled={submitting}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={!canSubmit}>
              {submitting ? 'Saving...' : 'Save backup'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
