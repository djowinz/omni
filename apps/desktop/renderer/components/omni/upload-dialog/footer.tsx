/**
 * UploadDialogFooter — Back / Cancel / primary CTA row.
 *
 * Chrome matches `step1-v3-outline-icons.html` and the per-step variants
 * across `step{1..4}*.html`. Cyan primary `#00D9FF` background with
 * `#09090B` text comes from INV-7.0.3.
 *
 * Per-step / per-state behaviour:
 *
 * | step    | state      | back | primary label |
 * |---------|------------|------|---------------|
 * | select  | idle       |  ✗   | Continue ›    |
 * | details | idle       |  ✓   | Continue ›    |
 * | packing | idle       |  ✓   | Publish ›     |
 * | upload  | in-flight  |  ✗   | (cancel only) |
 * | upload  | success    |  ✗   | Done          |
 * | upload  | error      |  ✓   | Retry ›       |
 *
 * INV-7.1.12 — no Back button on Step 1.
 * INV-7.4.5  — Step 4 success closes the dialog; error reveals Back+Retry.
 */

export type FooterStep = 'select' | 'details' | 'packing' | 'upload';
export type FooterState = 'idle' | 'in-flight' | 'success' | 'error';

export interface UploadDialogFooterProps {
  step: FooterStep;
  state: FooterState;
  primaryDisabled: boolean;
  onBack: () => void;
  onCancel: () => void;
  onPrimary: () => void;
}

function primaryLabelFor(step: FooterStep, state: FooterState): string {
  if (step === 'upload') {
    if (state === 'success') return 'Done';
    if (state === 'error') return 'Retry ›';
  }
  if (step === 'packing') return 'Publish ›';
  return 'Continue ›';
}

export function UploadDialogFooter({
  step,
  state,
  primaryDisabled,
  onBack,
  onCancel,
  onPrimary,
}: UploadDialogFooterProps) {
  // Step 1 has no Back (INV-7.1.12). Step 4 hides Back while uploading or
  // on success (only shows on error per INV-7.4.5).
  const showBack =
    step !== 'select' && !(step === 'upload' && (state === 'in-flight' || state === 'success'));

  const primaryLabel = primaryLabelFor(step, state);

  return (
    <div
      data-testid="upload-dialog-footer"
      className="flex shrink-0 items-center justify-between border-t border-[#27272A] pt-3"
    >
      {showBack ? (
        <button
          type="button"
          onClick={onBack}
          className="bg-transparent px-3 py-2 text-sm text-[#a1a1aa] hover:text-zinc-200"
        >
          Back
        </button>
      ) : (
        // Spacer so the right-aligned button group keeps its position when
        // Back is hidden. Avoids layout shift between steps.
        <div aria-hidden="true" />
      )}
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="bg-transparent px-3 py-2 text-sm text-[#a1a1aa] hover:text-zinc-200"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={onPrimary}
          disabled={primaryDisabled}
          className="rounded-md bg-[#00D9FF] px-4 py-2 text-sm font-semibold text-[#09090B] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {primaryLabel}
        </button>
      </div>
    </div>
  );
}
