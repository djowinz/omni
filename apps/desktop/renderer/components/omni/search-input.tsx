/**
 * SearchInput — debounced controlled text input for the Explore grid toolbar.
 *
 * The component itself is fully controlled (uncontrolled debounce happens
 * higher up at useExploreList level — 250ms via use-debounce).
 *
 * Per share-explorer-redesign spec §4.1 — pill style sized for the
 * h-16 toolbar context: 40px tall, #0D0D0F bg, #27272A border, lucide
 * Search icon leading.
 */

import { Search } from 'lucide-react';

export interface SearchInputProps {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
}

export function SearchInput({
  value,
  onChange,
  placeholder = 'Search themes and bundles…',
}: SearchInputProps) {
  return (
    <div className="flex h-10 max-w-[520px] flex-1 items-center gap-2.5 rounded-md border border-[#27272A] bg-[#0D0D0F] px-3.5">
      <Search className="h-4 w-4 flex-shrink-0 text-[#71717A]" aria-hidden />
      <input
        type="search"
        role="searchbox"
        aria-label="Search themes and bundles"
        placeholder={placeholder}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="flex-1 bg-transparent text-sm text-[#FAFAFA] outline-none placeholder:text-[#52525B]"
      />
    </div>
  );
}
