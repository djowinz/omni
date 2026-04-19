/**
 * UploadDialog — multi-step publish flow.
 *
 * Steps:
 *   1. Source picker  (skipped when sourcePath is prefilled)
 *   2. Review         (manifest fields via react-hook-form)
 *   3. Validate       (upload.pack dry-run → sanitize + size report)
 *   4. Publish        (upload.publish → live progress → success/error)
 *
 * First-publish gate: before the first publish action of a session, check
 * `identity.show.backed_up`. When false AND the user hasn't dismissed the
 * gate this session, open `<IdentityBackupDialog mode="first-publish">`
 * first; on success (or explicit skip) resume the publish. See plan §T6
 * Step 5 addendum for the exact flow. Fresh install / identity.show error
 * → proceed anyway; host will surface a structured error if it can't sign.
 *
 * Entry points:
 *   - widget-panel.tsx Publish button → UploadDialog with sourcePath=<active overlay>
 *   - explore-panel.tsx + Upload CTA  → UploadDialog with sourcePath=null (picker shown)
 */

import { useState, useEffect } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useShareWs } from '../../hooks/use-share-ws';
import { useWorkspaceList } from '../../hooks/use-workspace-list';
import {
  UploadFormSchema,
  DEFAULT_FORM,
  type UploadFormValues,
} from '../../lib/upload-form-schema';
import type {
  UploadPackResult,
  UploadPublishResult,
  UploadPublishProgress,
  ShareWsError,
} from '../../lib/share-types';
import { SourceStep, ReviewStep, ValidateStep, PublishStep } from './upload-dialog-steps';
import { IdentityBackupDialog } from './identity-backup-dialog';

export type UploadDialogMode = 'publish' | 'update';

export interface UploadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Workspace path to publish. null → show source picker. */
  sourcePath: string | null;
  /** Publish (new) or update (existing). */
  mode: UploadDialogMode;
  /** For mode='update' — the existing artifact id. */
  existingArtifactId?: string;
  /** Fired on successful publish so consumers can refresh their own state. */
  onPublished?: (result: UploadPublishResult['params']) => void;
  /**
   * Forwarded to the embedded IdentityBackupDialog's `saveBackup` prop so
   * tests can inject a mock persistence step. In production leave undefined —
   * the dialog falls back to the main-process save bridge (pending #016).
   */
  backupSaveBackup?: (encryptedBytes: Uint8Array) => Promise<string>;
}

type Step = 'source' | 'review' | 'validate' | 'publish';

export function UploadDialog({
  open,
  onOpenChange,
  sourcePath,
  mode,
  existingArtifactId,
  onPublished,
  backupSaveBackup,
}: UploadDialogProps) {
  const { send, subscribe } = useShareWs();
  const workspace = useWorkspaceList();

  const [step, setStep] = useState<Step>(sourcePath ? 'review' : 'source');
  const [chosenSource, setChosenSource] = useState<string | null>(sourcePath);

  const form = useForm<UploadFormValues>({
    resolver: zodResolver(UploadFormSchema),
    defaultValues: DEFAULT_FORM,
  });

  const [packing, setPacking] = useState(false);
  const [packResult, setPackResult] = useState<UploadPackResult | null>(null);
  const [packError, setPackError] = useState<ShareWsError | null>(null);

  const [progress, setProgress] = useState<UploadPublishProgress | null>(null);
  const [publishResult, setPublishResult] = useState<UploadPublishResult | null>(null);
  const [publishError, setPublishError] = useState<ShareWsError | null>(null);

  // First-publish gate state. skippedBackup is a session-lifetime flag; once
  // the user has either backed up OR explicitly dismissed the gate, we don't
  // re-prompt until the app restarts.
  const [backupDialogOpen, setBackupDialogOpen] = useState(false);
  const [skippedBackup, setSkippedBackup] = useState(false);

  // Subscribe to publish progress while this dialog is mounted.
  useEffect(() => {
    const unsub = subscribe('upload.publishProgress', (frame) => setProgress(frame));
    return unsub;
  }, [subscribe]);

  // Reset all multi-step state whenever the dialog closes. Without this the
  // "Artifact published" success screen from the previous session stays on
  // screen when the user reopens the dialog — they see the old artifact_id,
  // old status, old content_hash instead of a fresh Step 1 form. Resetting on
  // the close transition (not open-time) also preserves in-flight state if
  // the user accidentally reopens while a publish is racing.
  useEffect(() => {
    if (!open) {
      setStep(sourcePath ? 'review' : 'source');
      setChosenSource(sourcePath);
      setPacking(false);
      setPackResult(null);
      setPackError(null);
      setProgress(null);
      setPublishResult(null);
      setPublishError(null);
      setBackupDialogOpen(false);
      // skippedBackup is intentionally session-scoped — keep it across
      // open/close cycles so the user isn't re-prompted mid-session after
      // they already dismissed the first-publish gate once.
      form.reset(DEFAULT_FORM);
    }
  }, [open, sourcePath, form]);

  const handleSourceSelect = (path: string) => {
    setChosenSource(path);
  };

  const handlePack = async () => {
    if (!chosenSource) return;
    setPacking(true);
    setPackError(null);
    setPackResult(null);
    try {
      const resp = await send('upload.pack', { workspace_path: chosenSource });
      setPackResult(resp);
    } catch (err) {
      setPackError(err as ShareWsError);
    } finally {
      setPacking(false);
    }
  };

  /**
   * The actual publish/update dispatch. Kept separate from handlePublish so
   * the first-publish gate can pause and resume it via handleBackupResolved.
   */
  const doPublish = async () => {
    if (!chosenSource) return;
    const values = form.getValues();
    setPublishError(null);
    setProgress(null);
    try {
      const common = {
        workspace_path: chosenSource,
        bump: values.bump,
        name: values.name,
        description: values.description,
        tags: values.tags,
        license: values.license,
        version: values.version,
        omni_min_version: values.omni_min_version,
      };
      if (mode === 'update' && existingArtifactId) {
        const resp = await send('upload.update', {
          ...common,
          artifact_id: existingArtifactId,
        });
        // upload.update returns UploadUpdateResult whose status enum is a
        // superset of UploadPublishResult.status. Normalise to the publish
        // shape so PublishStep's success renderer can consume it uniformly;
        // the 'updated'/'unchanged' cases surface as the generic success view.
        const normalisedStatus =
          resp.params.status === 'created' || resp.params.status === 'deduplicated'
            ? resp.params.status
            : 'created';
        const normalised: UploadPublishResult = {
          id: resp.id,
          type: 'upload.publishResult',
          params: {
            artifact_id: resp.params.artifact_id,
            content_hash: resp.params.content_hash,
            status: normalisedStatus,
            worker_url: resp.params.worker_url,
          },
        };
        setPublishResult(normalised);
        onPublished?.(normalised.params);
      } else {
        const resp = await send('upload.publish', {
          ...common,
          visibility: 'public' as const,
        });
        setPublishResult(resp);
        onPublished?.(resp.params);
      }
    } catch (err) {
      setPublishError(err as ShareWsError);
    }
  };

  const handlePublish = async () => {
    if (!chosenSource) return;
    // Gate: if identity is unbacked AND user hasn't dismissed the gate this
    // session, open the IdentityBackupDialog first. On success → resume via
    // handleBackupSuccess. On skip → set session flag + resume via
    // handleBackupOpenChange.
    if (!skippedBackup) {
      try {
        const identity = await send('identity.show', {});
        if (!identity.params.backed_up) {
          setBackupDialogOpen(true);
          return; // gate — publish resumes after the dialog resolves
        }
      } catch {
        // If identity.show fails (fresh install / NOT_IMPLEMENTED stub),
        // proceed anyway — the host will surface a structured error on
        // publish if it can't sign.
      }
    }
    await doPublish();
  };

  const handleBackupSuccess = async (_backupPath: string) => {
    setBackupDialogOpen(false);
    // Mark as skipped so re-entering the flow doesn't re-prompt before the
    // host cache reflects the new backup state (identity.show updates are
    // post-#006; defensive treatment here avoids a stale-gate loop).
    setSkippedBackup(true);
    await doPublish();
  };

  const handleBackupOpenChange = async (nextOpen: boolean) => {
    setBackupDialogOpen(nextOpen);
    if (!nextOpen && !publishResult) {
      // Dialog closed without handleBackupSuccess firing → treat as skip.
      // Set session flag so the user isn't re-prompted on the same publish.
      setSkippedBackup(true);
      await doPublish();
    }
  };

  const handleNext = async () => {
    if (step === 'source' && chosenSource) {
      setStep('review');
      return;
    }
    if (step === 'review') {
      const valid = await form.trigger();
      if (!valid) return;
      setStep('validate');
      await handlePack();
      return;
    }
    if (step === 'validate' && packResult && !packError) {
      setStep('publish');
      await handlePublish();
      return;
    }
    if (step === 'publish' && publishResult) {
      onOpenChange(false);
    }
  };

  const handleBack = () => {
    if (step === 'review' && !sourcePath) setStep('source');
    else if (step === 'validate') setStep('review');
    else if (step === 'publish' && publishError) setStep('validate');
  };

  const nextDisabled =
    (step === 'source' && !chosenSource) ||
    (step === 'validate' && (packing || !packResult || !!packError));

  const nextLabel =
    step === 'source'
      ? 'Review'
      : step === 'review'
        ? 'Validate'
        : step === 'validate'
          ? mode === 'update'
            ? 'Update'
            : 'Publish'
          : publishResult
            ? 'Done'
            : 'Close';

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-xl" data-testid={`upload-step-${step}`}>
          <DialogHeader>
            <DialogTitle>
              {mode === 'update' ? 'Update artifact' : 'Publish to the Omni Hub'}
            </DialogTitle>
            <DialogDescription>
              Step {step === 'source' ? 1 : step === 'review' ? 2 : step === 'validate' ? 3 : 4} of
              4
            </DialogDescription>
          </DialogHeader>

          <div className="max-h-[60vh] overflow-y-auto py-2">
            {step === 'source' && (
              <SourceStep
                overlays={workspace.overlays}
                themes={workspace.themes}
                loading={workspace.loading}
                selected={chosenSource}
                onSelect={handleSourceSelect}
              />
            )}
            {step === 'review' && <ReviewStep form={form} />}
            {step === 'validate' && (
              <ValidateStep packing={packing} result={packResult} error={packError} />
            )}
            {step === 'publish' && (
              <PublishStep progress={progress} result={publishResult} error={publishError} />
            )}
          </div>

          <DialogFooter>
            {step !== 'source' && (
              <Button type="button" variant="outline" onClick={handleBack} disabled={packing}>
                Back
              </Button>
            )}
            <Button
              type="button"
              data-testid="upload-next-button"
              onClick={handleNext}
              disabled={nextDisabled}
            >
              {nextLabel}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <IdentityBackupDialog
        open={backupDialogOpen}
        onOpenChange={handleBackupOpenChange}
        onSuccess={handleBackupSuccess}
        mode="first-publish"
        saveBackup={backupSaveBackup}
      />
    </>
  );
}
