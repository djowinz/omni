import { useRef, useMemo, useState } from 'react';
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

export type IdentityBackupMode =
  | 'first-publish'
  | 'settings'
  | 'forced-rotation'
  | 'blocking-before-upload'
  | 'import';

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
  /** Optional callback for [Skip and publish anyway] in 'blocking-before-upload' mode. */
  onSkip?: () => void;
  /** When true (used by IdentityWelcomeDialog → import path in P12), the
   *  overwrite_existing checkbox is rendered checked + disabled. */
  lockOverwrite?: boolean;
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
  'blocking-before-upload': {
    title: 'Back up your identity first',
    description:
      'Before publishing for the first time, save an encrypted backup of your signing key. Without it you cannot recover your author identity if this device is lost.',
  },
  import: {
    title: 'Import existing identity',
    description: 'Restore a signing key from an encrypted .omniid backup.',
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

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 1) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
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
 * Default persistence implementation. Wired to the main-process
 * `identity:save-backup` IPC handler, which shows a native save dialog,
 * writes the encrypted bytes to the chosen path, and returns that path —
 * all in a single round-trip (see apps/desktop/main/main.ts). Tests + the
 * smoke page may still pass a `saveBackup` prop to stub this out.
 */
async function defaultSaveBackup(bytes: Uint8Array): Promise<string> {
  const save = window.omni?.saveIdentityBackup;
  if (!save) throw new Error('Save bridge not available');
  const path = await save(bytes);
  if (!path) throw new Error('Save cancelled');
  return path;
}

// ── ImportForm ────────────────────────────────────────────────────────────────

interface ImportFormProps {
  onSubmit: (params: { encryptedBytesB64: string; passphrase: string; overwriteExisting: boolean }) => void | Promise<void>;
  lockOverwrite: boolean;
  submitting: boolean;
  error: UserFacingError | null;
  onCancel: () => void;
  copyStatus: 'idle' | 'copied';
  onReport: () => void;
}

function ImportForm({
  onSubmit,
  lockOverwrite,
  submitting,
  error,
  onCancel,
  copyStatus,
  onReport,
}: ImportFormProps) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [passphrase, setPassphrase] = useState('');
  const [overwriteExisting, setOverwriteExisting] = useState(lockOverwrite);
  const [fileSelected, setFileSelected] = useState(false);

  const canSubmit = fileSelected && passphrase.length > 0 && !submitting;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    const file = fileRef.current?.files?.[0];
    if (!file) return;
    const arrayBuffer = await file.arrayBuffer();
    const bytes = new Uint8Array(arrayBuffer);
    const encryptedBytesB64 = bytesToBase64(bytes);
    await onSubmit({ encryptedBytesB64, passphrase, overwriteExisting });
  };

  return (
    <form className="flex flex-col gap-4 py-2" onSubmit={handleSubmit}>
      {/* Destructive banner */}
      <div
        role="note"
        className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/[0.08] p-3 text-xs leading-relaxed text-destructive"
      >
        <svg
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          className="mt-0.5 flex-shrink-0"
        >
          <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
          <line x1="12" y1="9" x2="12" y2="13" />
          <line x1="12" y1="17" x2="12.01" y2="17" />
        </svg>
        <span>
          <strong>Importing will replace your current signing key.</strong> Any overlays published
          with the current key can no longer be updated unless you also import that key later.
        </span>
      </div>

      <div className="flex flex-col gap-2">
        <Label htmlFor="identity-import-file">Backup file (.omniid)</Label>
        <Input
          id="identity-import-file"
          type="file"
          accept=".omniid"
          ref={fileRef}
          disabled={submitting}
          onChange={(e) => setFileSelected((e.target.files?.length ?? 0) > 0)}
        />
      </div>

      <div className="flex flex-col gap-2">
        <Label htmlFor="identity-import-passphrase">Passphrase</Label>
        <Input
          id="identity-import-passphrase"
          type="password"
          autoComplete="current-password"
          value={passphrase}
          onChange={(e) => setPassphrase(e.target.value)}
          disabled={submitting}
          autoFocus
        />
      </div>

      <div className="flex items-center gap-2">
        <input
          id="identity-import-overwrite"
          type="checkbox"
          checked={lockOverwrite || overwriteExisting}
          disabled={lockOverwrite || submitting}
          onChange={(e) => {
            if (!lockOverwrite) setOverwriteExisting(e.target.checked);
          }}
          aria-label="Overwrite existing identity"
        />
        <Label htmlFor="identity-import-overwrite">Overwrite existing identity</Label>
      </div>

      {error && (
        <div
          role="alert"
          className="flex flex-col gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
        >
          <span>{error.text}</span>
          <div className="flex items-center gap-2">
            <Button type="button" variant="outline" size="sm" onClick={onReport}>
              {copyStatus === 'copied' ? 'Copied' : 'Report this'}
            </Button>
          </div>
        </div>
      )}

      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          onClick={onCancel}
          disabled={submitting}
        >
          Cancel
        </Button>
        <Button type="submit" disabled={!canSubmit}>
          {submitting ? 'Importing...' : 'Import identity'}
        </Button>
      </DialogFooter>
    </form>
  );
}

// ── IdentityBackupDialog ──────────────────────────────────────────────────────

export function IdentityBackupDialog({
  open,
  onOpenChange,
  onSuccess,
  mode,
  saveBackup,
  onSkip,
  lockOverwrite,
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
        throw new Error(
          response.error?.message ?? response.error?.detail ?? 'identity.backup failed',
        );
      }
      const b64 = response.params?.encrypted_bytes_b64;
      if (typeof b64 !== 'string') {
        throw new Error('Malformed identity.backup response');
      }
      const bytes = base64ToBytes(b64);
      const path = await (saveBackup ?? defaultSaveBackup)(bytes);

      // Per spec T8: tell the host the backup landed on disk so backed_up
      // flips to true and the publish gate doesn't re-fire next session.
      // timestamp must be within ±86400s of now (Date.now() always is).
      await bridge({
        id: crypto.randomUUID(),
        type: 'identity.markBackedUp',
        params: { path, timestamp: Math.floor(Date.now() / 1000) },
      });

      reset();
      // Parent owns the close transition. We deliberately do NOT call
      // `onOpenChange(false)` here — calling both onSuccess AND
      // onOpenChange(false) caused a double-publish in the upload flow:
      // the upload-dialog wires both to `resolveBackupGate(...)`, so a
      // single successful backup fired `doPublish()` twice ~3ms apart,
      // which burned the daily upload quota and surfaced as a 429 cascade.
      // Both call sites (upload-dialog/index.tsx and __primitives-smoke.tsx)
      // close the dialog from inside their own onSuccess callback, so
      // dropping the redundant call is a no-op for them and a fix for
      // anyone who wires onOpenChange to a side-effect.
      onSuccess(path);
    } catch (err) {
      const mapped = toUserFacing(err);

      console.error('[identity-backup-dialog] save failed', mapped.opaquePayload);
      setError(mapped);
    } finally {
      setSubmitting(false);
    }
  };

  const handleImportSubmit = async (params: {
    encryptedBytesB64: string;
    passphrase: string;
    overwriteExisting: boolean;
  }) => {
    setSubmitting(true);
    setError(null);
    setCopyStatus('idle');
    try {
      const bridge = window.omni?.sendShareMessage;
      if (!bridge) throw new Error('IPC share bridge not available');
      const response = (await bridge({
        id: crypto.randomUUID(),
        type: 'identity.import',
        params: {
          encrypted_bytes_b64: params.encryptedBytesB64,
          passphrase: params.passphrase,
          overwrite_existing: params.overwriteExisting,
        },
      })) as {
        id?: string;
        type?: string;
        params?: { path?: unknown };
        error?: { message?: string; detail?: string };
      };
      if (response.type === 'error') {
        throw new Error(
          response.error?.message ?? response.error?.detail ?? 'identity.import failed',
        );
      }
      const path =
        typeof response.params?.path === 'string' ? response.params.path : 'imported';
      reset();
      onSuccess(path);
    } catch (err) {
      const mapped = toUserFacing(err);
      console.error('[identity-backup-dialog] import failed', mapped.opaquePayload);
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

        {mode === 'import' ? (
          <ImportForm
            onSubmit={handleImportSubmit}
            lockOverwrite={lockOverwrite ?? false}
            submitting={submitting}
            error={error}
            onCancel={() => handleOpenChange(false)}
            copyStatus={copyStatus}
            onReport={handleReport}
          />
        ) : (
          <form
            className="flex flex-col gap-4 py-2"
            onSubmit={(e) => {
              e.preventDefault();
              handleSubmit();
            }}
          >
            {mode === 'blocking-before-upload' && (
              <div
                role="note"
                className="flex items-start gap-2 rounded-md border border-[#fbbf24]/30 bg-[#f59e0b]/[0.08] p-3 text-xs leading-relaxed text-[#fbbf24]"
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  className="mt-0.5 flex-shrink-0"
                >
                  <circle cx="12" cy="12" r="10" />
                  <line x1="12" y1="8" x2="12" y2="12" />
                  <line x1="12" y1="16" x2="12.01" y2="16" />
                </svg>
                <span>
                  <strong className="text-[#fcd34d]">Why this blocks your first upload.</strong> If
                  you publish without a backup and lose access to this machine, your uploads stay
                  attributed to a key you cannot prove ownership of — and you cannot rotate or take
                  down your own work.
                </span>
              </div>
            )}

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
              {mode === 'blocking-before-upload' && onSkip && (
                <Button
                  type="button"
                  variant="link"
                  size="sm"
                  onClick={onSkip}
                  className="text-[#71717a] hover:text-[#a1a1aa]"
                >
                  Skip and publish anyway
                </Button>
              )}
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
        )}
      </DialogContent>
    </Dialog>
  );
}
