/**
 * TagPillList — multi-select pill list of allowed tags from config.vocab.
 *
 * Per share-explorer-redesign spec §4.3:
 * - border-radius:6px (rounded-md, our editor's standard pill radius)
 * - 11px font, 4×10 padding
 * - selected: cyan tint bg + cyan border + cyan text
 * - unselected: #27272A bg + #3F3F46 border + #D4D4D8 text
 * - loading: 6 muted-grey skeletons
 */

import { cn } from '@/lib/utils';

export interface TagPillListProps {
  tags: string[];
  selected: string[];
  onToggle: (tag: string) => void;
  loading?: boolean;
}

export function TagPillList({ tags, selected, onToggle, loading = false }: TagPillListProps) {
  if (loading) {
    return (
      <div className="flex flex-wrap gap-1.5">
        {Array.from({ length: 6 }).map((_, i) => (
          <span
            key={i}
            data-testid="tag-pill-skeleton"
            className="h-[22px] w-16 animate-pulse rounded-md bg-[#27272A]"
          />
        ))}
      </div>
    );
  }

  if (tags.length === 0) {
    return <div className="text-xs text-zinc-500">No tags available.</div>;
  }

  return (
    <div className="flex flex-wrap gap-1.5">
      {tags.map((tag) => {
        const isSelected = selected.includes(tag);
        return (
          <button
            key={tag}
            type="button"
            aria-pressed={isSelected}
            onClick={() => onToggle(tag)}
            className={cn(
              'rounded-md border px-2.5 py-1 text-[11px] transition-colors',
              isSelected
                ? 'border-[#00D9FF]/40 bg-[#00D9FF]/[0.10] text-[#00D9FF]'
                : 'border-[#3F3F46] bg-[#27272A] text-[#D4D4D8] hover:border-[#52525B]',
            )}
          >
            {tag}
          </button>
        );
      })}
    </div>
  );
}
