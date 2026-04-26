/**
 * Review — Step 2 of the redesigned Upload dialog.
 *
 * Renders a vertical field stack per spec §7.2 + INV-7.5.2:
 *   1. Name (required, red `*` after the label)
 *   2. Version Bump Select (only when `state.mode === 'update'`) —
 *      Patch / Minor / Major; default Patch.
 *   3. Description (optional — NO asterisk)
 *   4. Preview Image — `ReviewPreviewImage` (INV-7.2.4) wired here in
 *      OWI-53 (Wave B Task B1.2). The component owns its 5-state visual
 *      machine + IDB persistence + ONNX RPC; this file just threads in
 *      `overlayPath` (= `selected.workspace_path`) and the auto-preview
 *      `file://` URL.
 *   5. Tags — `ReviewTagBadges` flex-wrap pills.
 *   6. License — `ReviewLicenseSelect` with optional Custom identifier.
 *   7. Policy Disclosure — `ReviewPolicyDisclosure` block.
 *
 * Form integration uses `react-hook-form` mirroring the legacy
 * `ReviewStep` (`register` / `setValue` / `watch` / `errors`) so this drop-in
 * stays compatible with `upload-dialog.tsx`'s existing form orchestration.
 */

import type { PublishablesEntry } from '@omni/shared-types';
import type { UseFormReturn } from 'react-hook-form';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { UploadFormValues } from '@/lib/upload-form-schema';
import { ReviewLicenseSelect } from './review-license-select';
import { ReviewPolicyDisclosure } from './review-policy-disclosure';
import { ReviewPreviewImage } from './review-preview-image';
import { ReviewTagBadges } from './review-tag-badges';

/**
 * Update-mode bump options. Differs from the legacy `BUMP_OPTIONS` constant —
 * the new dialog hides the `None` choice (INV-7.5.2: Patch / Minor / Major only)
 * and defaults to `Patch`.
 */
const UPDATE_BUMP_OPTIONS: { value: UploadFormValues['bump']; label: string }[] = [
  { value: 'patch', label: 'Patch' },
  { value: 'minor', label: 'Minor' },
  { value: 'major', label: 'Major' },
];

export interface ReviewProps {
  state: {
    mode: 'create' | 'update';
    /**
     * Currently-selected workspace entry. Threaded from the upload machine
     * (`UploadMachineState.selected`) so the Preview Image field can derive
     * its IndexedDB key (`workspace_path`) and the auto-generated preview
     * `file://` URL (gated on `has_preview`). Optional so existing test
     * harnesses that don't exercise Preview Image can omit it — when
     * absent or `null`, the slot renders an empty placeholder div instead
     * of the full `ReviewPreviewImage` (production code always passes the
     * machine's `state.selected`).
     */
    selected?: PublishablesEntry | null;
  };
  form: UseFormReturn<UploadFormValues>;
}

/**
 * Resolve the auto-preview URL for an entry, mirroring the convention in
 * `source-picker-list-row.tsx::previewUrlFor`. Returns `null` when the entry
 * has no save-time rendered preview — the Preview Image field then shows the
 * zinc-gradient placeholder per INV-7.2.4 default state.
 *
 * URLs use the `omni-preview://` scheme registered in
 * `apps/desktop/main/main.ts`, which maps `omni-preview://<segment>/<rest>`
 * to `<userData>/<segment>/<rest>` on disk.
 *
 * Overlay preview path: `<data_dir>/overlays/<name>/.omni-preview.png`.
 * Theme preview path:   `<data_dir>/themes/<base>.preview.png` where `<base>`
 * is the theme filename minus its `.css` extension.
 */
function resolveAutoPreviewSrc(entry: PublishablesEntry | null): string | null {
  if (!entry || !entry.has_preview) return null;
  if (entry.kind === 'overlay') {
    return `omni-preview://${entry.workspace_path}/.omni-preview.png`;
  }
  const base = entry.workspace_path.replace(/\.css$/i, '');
  return `omni-preview://${base}.preview.png`;
}

export function Review({ state, form }: ReviewProps) {
  const {
    register,
    setValue,
    watch,
    formState: { errors },
  } = form;
  const bump = watch('bump');

  return (
    <div data-testid="review-step" className="flex flex-col gap-3.5">
      {/* Name — required */}
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="upload-name">
          Name <span className="text-[#f43f5e]">*</span>
        </Label>
        <Input
          id="upload-name"
          data-testid="upload-name"
          placeholder="My overlay"
          {...register('name')}
        />
        {errors.name ? <p className="text-xs text-rose-400">{errors.name.message}</p> : null}
      </div>

      {/* Version Bump — update mode only (INV-7.5.2) */}
      {state.mode === 'update' ? (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="upload-bump">Version Bump</Label>
          <Select
            value={bump}
            onValueChange={(v) =>
              setValue('bump', v as UploadFormValues['bump'], {
                shouldValidate: false,
                shouldDirty: true,
              })
            }
          >
            <SelectTrigger
              id="upload-bump"
              data-testid="upload-bump"
              className="w-full bg-[#0A0A0B] text-[#FAFAFA] sm:w-auto sm:min-w-[160px]"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {UPDATE_BUMP_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      ) : null}

      {/* Description — optional, NO asterisk (INV-7.2.3) */}
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="upload-description">Description</Label>
        <Textarea
          id="upload-description"
          data-testid="upload-description"
          rows={3}
          placeholder="Describe your overlay…"
          {...register('description')}
        />
        {errors.description ? (
          <p className="text-xs text-rose-400">{errors.description.message}</p>
        ) : null}
      </div>

      {/* Preview Image (INV-7.2.4) — wired in OWI-53 / Task B1.2.
          `overlayPath` is the IDB key for any user-uploaded custom preview;
          `autoPreviewSrc` is the `file://` URL of the host's save-time
          rendered `.omni-preview.png` (null when `has_preview === false`,
          which makes the component fall back to the zinc-gradient AUTO
          placeholder). When `selected` is `null` (shouldn't happen on
          Step 2, but typed for safety) the slot stays empty. */}
      {state.selected ? (
        <ReviewPreviewImage
          overlayPath={state.selected.workspace_path}
          autoPreviewSrc={resolveAutoPreviewSrc(state.selected)}
        />
      ) : (
        <div data-testid="review-preview-image-slot" />
      )}

      {/* Tags */}
      <div className="flex flex-col gap-1.5">
        <Label>Tags</Label>
        <ReviewTagBadges form={form} />
      </div>

      {/* License + (conditional) Custom identifier input */}
      <ReviewLicenseSelect form={form} />

      {/* Policy disclosure */}
      <ReviewPolicyDisclosure />
    </div>
  );
}
