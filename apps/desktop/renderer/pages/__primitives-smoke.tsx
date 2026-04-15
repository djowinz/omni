// Developer smoke harness for the shared UI primitives (#020).
// NOT part of the production user flow. Filename prefix `__` is the convention
// for internal developer pages; not linked from any user-visible navigation.
// Removable after Phase 3 specs wire these primitives into real flows.

import { useState } from 'react';

import { FingerprintDisplay } from '@/components/omni/fingerprint-display';
import {
  IdentityBackupDialog,
  type IdentityBackupMode,
} from '@/components/omni/identity-backup-dialog';
import { PasswordStrengthMeter } from '@/components/ui/password-strength-meter';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { toast } from '@/lib/toast';

const SAMPLE_WORDS: [string, string, string] = ['alpha', 'bravo', 'charlie'];
const SAMPLE_EMOJI: [string, string, string, string, string, string] = [
  '🌟',
  '🌙',
  '🔥',
  '💧',
  '🌈',
  '⚡',
];

// Mock saveBackup so the success flow completes without the #016 IPC bridge.
async function mockSaveBackup(_bytes: Uint8Array): Promise<string> {
  return '/mock/backup.omniid';
}

export default function PrimitivesSmoke() {
  const [password, setPassword] = useState('');
  const [dialogMode, setDialogMode] = useState<IdentityBackupMode | null>(null);

  const openDialog = (mode: IdentityBackupMode) => setDialogMode(mode);

  return (
    <div className="max-w-2xl mx-auto p-8 space-y-8">
      <header className="space-y-1">
        <h1 className="text-2xl font-semibold">Primitives smoke harness</h1>
        <p className="text-sm text-muted-foreground">
          Developer-only page for manually exercising the shared UI primitives
          shipped in sub-spec #020. Not linked from any user-facing navigation.
        </p>
      </header>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold">FingerprintDisplay — inline</h2>
        <FingerprintDisplay
          variant="inline"
          words={SAMPLE_WORDS}
          emoji={SAMPLE_EMOJI}
          showEmoji={true}
        />
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold">FingerprintDisplay — block</h2>
        <FingerprintDisplay
          variant="block"
          words={SAMPLE_WORDS}
          emoji={SAMPLE_EMOJI}
          showEmoji={true}
        />
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold">PasswordStrengthMeter</h2>
        <div className="flex flex-col gap-2">
          <Label htmlFor="smoke-password">Password</Label>
          <Input
            id="smoke-password"
            type="text"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="Type to watch weak → medium → strong"
          />
          <PasswordStrengthMeter value={password} minLength={12} />
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold">IdentityBackupDialog</h2>
        <div className="flex flex-wrap gap-2">
          <Button type="button" onClick={() => openDialog('first-publish')}>
            Open (first-publish)
          </Button>
          <Button type="button" onClick={() => openDialog('settings')}>
            Open (settings)
          </Button>
          <Button type="button" onClick={() => openDialog('forced-rotation')}>
            Open (forced-rotation)
          </Button>
        </div>
        {dialogMode !== null && (
          <IdentityBackupDialog
            open={dialogMode !== null}
            onOpenChange={(next) => {
              if (!next) setDialogMode(null);
            }}
            onSuccess={(path) => {
              toast.success(`Backup saved to ${path}`);
              setDialogMode(null);
            }}
            mode={dialogMode}
            saveBackup={mockSaveBackup}
          />
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold">Toast notifications</h2>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            onClick={() => toast.success('This is a success toast')}
          >
            Trigger success toast
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() =>
              toast.error({
                code: 'SAMPLE_ERROR',
                kind: 'HostLocal',
                detail: 'DETAIL-NOT-FOR-UI',
                message:
                  'Sample error — the text you see should be this message, not the detail above.',
              })
            }
          >
            Trigger error toast
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          Expected: the error toast surfaces the <code>message</code>, never the{' '}
          <code>detail</code>; its &ldquo;Report this&rdquo; action copies the
          opaque payload (including the detail) to the clipboard.
        </p>
      </section>
    </div>
  );
}
