/**
 * SortDropdown — pill-style sort selector for the Explore grid toolbar.
 *
 * Per share-explorer-redesign spec §4.2: 4 user-facing labels mapping to 3
 * wire enum values. "Newest" and "Recently Updated" both map to "new"
 * because the shipped ExplorerListParamsSchema.sort enum has 3 values
 * (`new` / `installs` / `name`); the worker sorts `new` by `updated_at DESC`
 * which is what both labels mean today. If users want them distinguished,
 * that's a separate worker contract change (out of scope for this spec).
 */

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { SlidersHorizontal } from 'lucide-react';
import type { ExploreSort } from '../../hooks/use-explore-filters';

interface Option {
  label: string;
  value: ExploreSort;
}

const OPTIONS: Option[] = [
  { label: 'Newest', value: 'new' },
  { label: 'Most Popular', value: 'installs' },
  { label: 'Recently Updated', value: 'new' },
  { label: 'A–Z', value: 'name' },
];

const VALUE_TO_DISPLAY_LABEL: Record<ExploreSort, string> = {
  new: 'Newest',
  installs: 'Most Popular',
  name: 'A–Z',
};

export interface SortDropdownProps {
  value: ExploreSort;
  onChange: (next: ExploreSort) => void;
}

export function SortDropdown({ value, onChange }: SortDropdownProps) {
  // Radix Select uses string values; we encode the index of the option so
  // "Newest" and "Recently Updated" don't collide on the same value="new".
  // The visible trigger label uses the canonical mapping (via wire enum).
  return (
    <Select
      value={String(OPTIONS.findIndex((o) => o.value === value && o.label === VALUE_TO_DISPLAY_LABEL[value]))}
      onValueChange={(idxStr) => {
        const idx = Number(idxStr);
        const opt = OPTIONS[idx];
        if (opt) onChange(opt.value);
      }}
    >
      <SelectTrigger
        className="flex h-10 items-center gap-2 rounded-md border border-[#27272A] bg-[#0D0D0F] px-3.5 text-sm text-[#A1A1AA]"
        aria-label="Sort"
      >
        <SlidersHorizontal className="h-3.5 w-3.5" aria-hidden />
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {OPTIONS.map((opt, idx) => (
          <SelectItem key={`${opt.label}-${idx}`} value={String(idx)}>
            {opt.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
