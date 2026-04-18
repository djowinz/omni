/**
 * ExploreEmptyState — friendly empty-state card shown when a sub-tab or
 * filtered query produces zero results. One component handles every empty
 * case (filters matched nothing, Installed tab, My Uploads tab) — callers
 * pass the label.
 */

import { Compass } from 'lucide-react';

export interface ExploreEmptyStateProps {
  label: string;
  hint?: string;
}

export function ExploreEmptyState({ label, hint }: ExploreEmptyStateProps) {
  return (
    <div
      data-testid="explore-grid-empty"
      className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center"
    >
      <Compass className="h-8 w-8 text-zinc-600" aria-hidden />
      <p className="text-sm text-zinc-400">{label}</p>
      {hint ? <p className="text-xs text-zinc-500">{hint}</p> : null}
    </div>
  );
}
