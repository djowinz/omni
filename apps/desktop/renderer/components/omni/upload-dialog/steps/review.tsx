/**
 * Review — Step 2 of the redesigned Upload dialog.
 *
 * Renders a vertical field stack per spec §7.2 + INV-7.5.2:
 *   1. Name (required, red `*` after the label)
 *   2. Version Bump Select (only when `state.mode === 'update'`) —
 *      Patch / Minor / Major; default Patch.
 *   3. Description (optional — NO asterisk)
 *   4. Preview Image slot — placeholder div for Wave A; the real field
 *      lands in Wave B (OWI-52, B1.1–3).
 *   5. Tags — `ReviewTagBadges` flex-wrap pills.
 *   6. License — `ReviewLicenseSelect` with optional Custom identifier.
 *   7. Policy Disclosure — `ReviewPolicyDisclosure` block.
 *
 * Form integration uses `react-hook-form` mirroring the legacy
 * `ReviewStep` (`register` / `setValue` / `watch` / `errors`) so this drop-in
 * stays compatible with `upload-dialog.tsx`'s existing form orchestration.
 */

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
  };
  form: UseFormReturn<UploadFormValues>;
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

      {/* Preview Image — owned by Wave B (OWI-52, B1.1-3) */}
      <div data-testid="review-preview-image-slot" />

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
