/**
 * ReviewLicenseSelect — Radix Select dropdown surfacing the SPDX list defined
 * in `LICENSE_OPTIONS`. When the user picks `Custom`, a free-text Input
 * appears below the Select for a free-form license identifier (stored in
 * the form's `customLicense` field). Spec INV-7.2.6.
 *
 * Form integration uses `setValue` with `shouldValidate: false, shouldDirty: true`
 * — same pattern as `ReviewTagBadges` so toggling the dropdown doesn't cascade
 * Name-required errors before the Name field is touched.
 */

import type { UseFormReturn } from 'react-hook-form';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { LICENSE_OPTIONS, type UploadFormValues } from '@/lib/upload-form-schema';

export interface ReviewLicenseSelectProps {
  form: UseFormReturn<UploadFormValues>;
}

export function ReviewLicenseSelect({ form }: ReviewLicenseSelectProps) {
  const {
    register,
    setValue,
    watch,
    formState: { errors },
  } = form;
  const license = watch('license') ?? '';
  const isCustom = license === 'Custom';

  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor="upload-license">License</Label>
      <Select
        value={license || undefined}
        onValueChange={(v) =>
          setValue('license', v, { shouldValidate: false, shouldDirty: true })
        }
      >
        <SelectTrigger
          id="upload-license"
          data-testid="review-license-trigger"
          className="w-full bg-[#0A0A0B] text-[#FAFAFA] sm:w-auto sm:min-w-[140px]"
        >
          <SelectValue placeholder="Select a license" />
        </SelectTrigger>
        <SelectContent>
          {LICENSE_OPTIONS.map((opt) => (
            <SelectItem
              key={opt.value}
              value={opt.value}
              data-testid={`review-license-option-${opt.value}`}
            >
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {isCustom ? (
        <div className="flex flex-col gap-1">
          <Label htmlFor="upload-custom-license">Custom license identifier</Label>
          <Input
            id="upload-custom-license"
            data-testid="review-custom-license-input"
            placeholder="e.g. proprietary, see LICENSE.txt"
            {...register('customLicense')}
          />
          {errors.customLicense ? (
            <p className="text-xs text-rose-400">{errors.customLicense.message}</p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
