/**
 * ReviewTagBadges — flex-wrap row of Badge-style pills for Step 2's Tag field.
 *
 * Spec INV-7.2.5:
 *  - Selected pill: 4×10 padding, 6px radius, border `#00D9FF`,
 *    bg `rgba(0, 217, 255, 0.12)`, text `#FAFAFA`, font 12px.
 *  - Unselected pill: same geometry, border `#27272A`, transparent bg,
 *    text `#a1a1aa`.
 *  - Click toggles selection without cascading whole-form validation
 *    (`shouldValidate: false, shouldDirty: true`) — matches the legacy
 *    `ReviewStep::toggleTag` behavior so the Name-required error doesn't
 *    fire before the user has touched the Name field.
 */

import type { UseFormReturn } from 'react-hook-form';
import { useConfigVocab } from '@/hooks/use-config-vocab';
import { cn } from '@/lib/utils';
import type { UploadFormValues } from '@/lib/upload-form-schema';

export interface ReviewTagBadgesProps {
  form: UseFormReturn<UploadFormValues>;
  /** Optional override — primarily for tests that don't want to mock useConfigVocab. */
  tags?: string[];
}

export function ReviewTagBadges({ form, tags: tagsOverride }: ReviewTagBadgesProps) {
  const vocab = useConfigVocab();
  const tags = tagsOverride ?? vocab.tags;
  const loading = tagsOverride ? false : vocab.loading;

  const { setValue, watch } = form;
  const selected = watch('tags') ?? [];

  const toggle = (tag: string) => {
    const next = selected.includes(tag)
      ? selected.filter((t) => t !== tag)
      : [...selected, tag];
    // shouldValidate: false — see file-level note. The Step 2 Continue
    // handler already calls form.trigger() before advancing.
    setValue('tags', next, { shouldValidate: false, shouldDirty: true });
  };

  if (loading) {
    return (
      <p data-testid="review-tag-badges-loading" className="text-xs text-zinc-500">
        Loading tags…
      </p>
    );
  }

  return (
    <div
      data-testid="review-tag-badges"
      className="flex flex-wrap gap-1.5"
    >
      {tags.map((tag) => {
        const isSelected = selected.includes(tag);
        return (
          <button
            key={tag}
            type="button"
            data-testid={`review-tag-badge-${tag}`}
            data-selected={isSelected ? 'true' : 'false'}
            onClick={() => toggle(tag)}
            className={cn(
              'cursor-pointer rounded-md border px-2.5 py-1 text-xs transition-colors',
              isSelected
                ? 'border-[#00D9FF] bg-[#00D9FF]/[0.12] text-[#FAFAFA]'
                : 'border-[#27272A] bg-transparent text-zinc-400 hover:text-zinc-300',
            )}
          >
            {tag}
          </button>
        );
      })}
    </div>
  );
}
