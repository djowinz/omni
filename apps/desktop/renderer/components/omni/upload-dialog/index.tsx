/**
 * UploadDialog — Step 1 → Step 4 publish flow (new shell).
 *
 * Replaces the legacy `apps/desktop/renderer/components/omni/upload-dialog.tsx`
 * (deleted in T-A2.1, OWI-41). This file owns the dialog chrome (header /
 * stepper / scrollable content / footer) and routes the active step to the
 * right step component.
 *
 * Structural invariants (INV-7.0.*):
 *   - DialogContent: `sm:max-w-xl max-h-[90vh] flex flex-col`
 *   - 3 children: header (`shrink-0`), content region (`flex-1 min-h-0
 *     overflow-y-auto py-2`), footer (`shrink-0`). Stepper sits inside the
 *     header band (`shrink-0` + `my-3`).
 *   - Cyan primary `#00D9FF`, ghost zinc text — see `footer.tsx`.
 *
 * Step components (`SourcePicker`, `Review`, `Packing`, `Publish`) ship as
 * inline stubs in this commit (OWI-35 / Task A1.1) so typecheck passes
 * standalone. The 4 sibling Wave A1 tasks (A1.2–A1.5 / OWI-36–OWI-39) write
 * the real components into `./steps/*.tsx` in parallel; T-A2.1 (OWI-41)
 * swaps these inline stubs for `import { … } from './steps/*'`.
 *
 * State + transitions live in `./hooks/use-upload-machine`. The dialog
 * resets the machine whenever `open` flips to `false` so a partial upload
 * doesn't leak into the next session.
 */

import { useEffect } from 'react';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Stepper } from './stepper';
import { UploadDialogHeader } from './header';
import { UploadDialogFooter } from './footer';
import {
  useUploadMachine,
  type UploadMachineActions,
  type UploadMachineState,
} from './hooks/use-upload-machine';

// ── Inline step stubs ────────────────────────────────────────────────────────
//
// Replaced by `./steps/*` in T-A2.1 once A1.2–A1.5 land. Until then these
// stubs let `pnpm --filter @omni/desktop typecheck` pass on this commit
// alone — see file header for the swap-in rationale.

interface StepProps {
  state: UploadMachineState;
  actions: UploadMachineActions;
}

function SourcePicker(_props: StepProps) {
  return <div data-testid="step-source-picker-stub" />;
}

function Review(_props: StepProps) {
  return <div data-testid="step-review-stub" />;
}

function Packing(_props: StepProps) {
  return <div data-testid="step-packing-stub" />;
}

function Publish(_props: StepProps) {
  return <div data-testid="step-publish-stub" />;
}

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
   * Step 1's source picker.
   */
  prefilledPath?: string | null;
}

// ── Component ────────────────────────────────────────────────────────────────

export function UploadDialog({
  open,
  onOpenChange,
  prefilledPath: _prefilledPath,
}: UploadDialogProps) {
  const { state, actions } = useUploadMachine();
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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent showCloseButton={false} className="flex max-h-[90vh] flex-col sm:max-w-xl">
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
          {state.step === 'select' && <SourcePicker state={state} actions={actions} />}
          {state.step === 'details' && <Review state={state} actions={actions} />}
          {state.step === 'packing' && <Packing state={state} actions={actions} />}
          {state.step === 'upload' && <Publish state={state} actions={actions} />}
        </div>

        <UploadDialogFooter
          step={state.step}
          state={state.uploadState}
          primaryDisabled={state.primaryDisabled}
          onBack={actions.back}
          onCancel={() => onOpenChange(false)}
          onPrimary={() => {
            void actions.next();
          }}
        />
      </DialogContent>
    </Dialog>
  );
}
