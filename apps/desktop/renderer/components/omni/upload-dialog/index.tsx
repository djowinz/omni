/**
 * UploadDialog — Step 1 → Step 4 publish flow.
 *
 * Replaces the legacy `apps/desktop/renderer/components/omni/upload-dialog.tsx`
 * monolith (deleted in T-A2.1, OWI-41). This file owns the dialog chrome
 * (header / stepper / scrollable content / footer) and routes the active
 * step to its real component under `./steps/*`.
 *
 * Structural invariants (INV-7.0.*):
 *   - DialogContent: `sm:max-w-xl max-h-[90vh] flex flex-col`
 *   - 3 children: header (`shrink-0`), content region (`flex-1 min-h-0
 *     overflow-y-auto py-2`), footer (`shrink-0`). Stepper sits inside the
 *     header band (`shrink-0` + `my-3`).
 *   - Cyan primary `#00D9FF`, ghost zinc text — see `footer.tsx`.
 *
 * State + transitions live in `./hooks/use-upload-machine`. The dialog
 * resets the machine whenever `open` flips to `false` so a partial upload
 * doesn't leak into the next session.
 *
 * First-publish backup gate: `useUploadMachine` raises `state.backupGateOpen`
 * before publish when the running identity isn't backed up; this dialog
 * mounts `<IdentityBackupDialog mode="first-publish">` in that case.
 * Resolving the backup dialog calls `actions.resolveBackupGate(...)` which
 * marks the gate as dismissed for the session and resumes the publish.
 */

import { useEffect } from 'react';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import type { UploadPublishResult } from '../../../lib/share-types';
import { Stepper } from './stepper';
import { UploadDialogHeader } from './header';
import { UploadDialogFooter } from './footer';
import {
  useUploadMachine,
  type UploadMachineState,
} from './hooks/use-upload-machine';
import { SourcePicker } from './steps/source-picker';
import { Review } from './steps/review';
import { Packing } from './steps/packing';
import { Publish } from './steps/publish';
import { IdentityBackupDialog } from '../identity-backup-dialog';

// ── Stepper config ───────────────────────────────────────────────────────────

const STEP_LABELS = ['Select', 'Details', 'Packing', 'Upload'] as const;
const STEP_INDEX: Record<UploadMachineState['step'], number> = {
  select: 0,
  details: 1,
  packing: 2,
  upload: 3,
};

// ── Public props ─────────────────────────────────────────────────────────────

export interface UploadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /**
   * Optional pre-selected workspace path (e.g. when launched from
   * widget-panel.tsx's "Publish" button). When absent the user picks via
   * Step 1's source picker. The machine resolves this to the matching
   * `PublishablesEntry` after `workspace.listPublishables` returns and
   * jumps the dialog directly to Step 2.
   */
  prefilledPath?: string | null;
  /** Fired on successful publish so consumers can refresh their own state. */
  onPublished?: (result: UploadPublishResult['params']) => void;
  /**
   * Forwarded to the embedded IdentityBackupDialog's `saveBackup` prop so
   * tests can inject a mock persistence step. In production leave undefined —
   * the dialog falls back to the main-process save bridge (pending #016).
   */
  backupSaveBackup?: (encryptedBytes: Uint8Array) => Promise<string>;
}

// ── Component ────────────────────────────────────────────────────────────────

export function UploadDialog({
  open,
  onOpenChange,
  prefilledPath = null,
  onPublished,
  backupSaveBackup,
}: UploadDialogProps) {
  const { state, actions, form } = useUploadMachine({ prefilledPath, onPublished });
  const stepIdx = STEP_INDEX[state.step];

  // Reset on close — a partial upload session must not leak into the next
  // open. `actions.reset` is referentially stable (useCallback) so this
  // effect only runs when `open` changes.
  useEffect(() => {
    if (!open) {
      actions.reset();
    }
  }, [open, actions]);

  // Narrow the selected entry's `kind` to the header's expected union.
  // PublishablesEntry.kind is `string` (forward-compat) but today's worker
  // emits only "overlay" | "theme"; anything else falls through as `null`
  // and the header defaults to "Overlay" (INV-7.0.4).
  const headerKind: 'overlay' | 'theme' | null =
    state.selected?.kind === 'theme'
      ? 'theme'
      : state.selected?.kind === 'overlay'
        ? 'overlay'
        : null;

  // Step 4 'Done' click closes the dialog. The footer renders 'Done' as the
  // primary on uploadState='success'; that primary fires `actions.next()`
  // through `onPrimary` below. We intercept here so `next()` doesn't try to
  // step past the upload state.
  const handlePrimary = () => {
    if (state.step === 'upload' && state.uploadState === 'success') {
      onOpenChange(false);
      return;
    }
    if (state.step === 'upload' && state.uploadState === 'error') {
      // Retry — re-fire the publish via the recovery action that already
      // exists for the AuthorNameConflict card. Calling `linkAndUpdate`
      // would force update mode; instead we go back to packing and let
      // the user click Publish again.
      actions.back();
      return;
    }
    void actions.next();
  };

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent
          showCloseButton={false}
          className="flex max-h-[90vh] flex-col sm:max-w-xl"
          data-testid="upload-dialog-content"
        >
          <UploadDialogHeader mode={state.mode} kind={headerKind} />

          <div className="my-3 shrink-0">
            <Stepper
              steps={[...STEP_LABELS]}
              current={stepIdx}
              completed={state.completedSteps}
              error={state.stepError}
            />
          </div>

          <div className="flex-1 min-h-0 overflow-y-auto py-2">
            {state.step === 'select' && (
              <SourcePicker
                state={{
                  selectedKind: state.selectedKind,
                  selected: state.selected,
                  mode: state.mode,
                  currentPubkey: state.currentPubkey,
                }}
                actions={{
                  selectKind: actions.selectKind,
                  selectItem: actions.selectItem,
                }}
              />
            )}
            {state.step === 'details' && <Review state={{ mode: state.mode }} form={form} />}
            {state.step === 'packing' && (
              <Packing
                actions={{ retry: () => void actions.retryPack() }}
                violations={state.packViolations}
              />
            )}
            {state.step === 'upload' && (
              <Publish
                artifactName={form.getValues('name')}
                state={{
                  // Machine uses `idle | in-flight | success | error`; the
                  // Publish step models the user-visible spinner only and
                  // collapses idle + in-flight into 'uploading'. Idle on
                  // step 4 is a transient state between PUBLISH_START and
                  // the first publishProgress frame; rendering 'uploading'
                  // chrome is correct UX (the spinner shows immediately).
                  uploadState:
                    state.uploadState === 'success'
                      ? 'success'
                      : state.uploadState === 'error'
                        ? 'error'
                        : 'uploading',
                  progress: state.publishProgress,
                  result: state.publishResult,
                  error: state.publishError,
                }}
                actions={{
                  linkAndUpdate: (artifactId) => void actions.linkAndUpdate(artifactId),
                  renameAndPublishNew: actions.renameAndPublishNew,
                }}
              />
            )}
          </div>

          <UploadDialogFooter
            step={state.step}
            state={state.uploadState}
            primaryDisabled={state.primaryDisabled}
            onBack={actions.back}
            onCancel={() => onOpenChange(false)}
            onPrimary={handlePrimary}
          />
        </DialogContent>
      </Dialog>

      <IdentityBackupDialog
        open={state.backupGateOpen}
        onOpenChange={(nextOpen) => {
          // Closed without success → treat as dismiss; resume publish.
          if (!nextOpen) {
            void actions.resolveBackupGate(true);
          }
        }}
        onSuccess={() => {
          void actions.resolveBackupGate(false);
        }}
        mode="first-publish"
        saveBackup={backupSaveBackup}
      />
    </>
  );
}
