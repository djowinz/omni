/**
 * ExploreSidebar — 260px sidebar with tab nav + Type filter + Tag pills.
 *
 * Per share-explorer-redesign spec §3.2:
 *   1. Tab nav   — Discover / Installed / My Uploads (cyan-tinted selected)
 *   2. TYPE      — All / Themes / Bundles (no counts)
 *   3. TAGS      — multi-select via <TagPillList />
 *
 * Sort radio is gone (moved to grid toolbar in Task 11).
 */

import { Compass, Download, Upload as UploadIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import {
  useExploreFilters,
  type ExploreKind,
  type ExploreTab,
} from '../../hooks/use-explore-filters';
import { useConfigVocab } from '../../hooks/use-config-vocab';
import { TagPillList } from './tag-pill-list';

const TABS: { id: ExploreTab; label: string; icon: typeof Compass }[] = [
  { id: 'discover', label: 'Discover', icon: Compass },
  { id: 'installed', label: 'Installed', icon: Download },
  { id: 'my-uploads', label: 'My Uploads', icon: UploadIcon },
];

const KINDS: { value: ExploreKind; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'theme', label: 'Themes' },
  { value: 'bundle', label: 'Bundles' },
];

export function ExploreSidebar() {
  const { tab, kind, tags, setTab, setKind, setTags } = useExploreFilters();
  const vocab = useConfigVocab();

  const toggleTag = (tag: string) => {
    if (tags.includes(tag)) {
      setTags(tags.filter((t) => t !== tag));
    } else {
      setTags([...tags, tag]);
    }
  };

  return (
    <aside
      data-testid="explore-sidebar"
      className="flex w-[260px] flex-shrink-0 flex-col gap-5 overflow-y-auto border-r border-[#27272A] bg-[#141416] p-3.5"
    >
      {/* Tab nav */}
      <nav className="flex flex-col gap-0.5">
        {TABS.map((t) => {
          const Icon = t.icon;
          const active = tab === t.id;
          return (
            <button
              key={t.id}
              data-testid={`explore-sidebar-tab-${t.id}`}
              onClick={() => setTab(t.id)}
              className={cn(
                'flex items-center gap-3 rounded-md border px-3.5 py-2.5 text-left text-sm transition-colors',
                active
                  ? 'border-[#00D9FF]/25 bg-[#00D9FF]/[0.08] text-[#00D9FF]'
                  : 'border-transparent text-[#A1A1AA] hover:bg-[#27272A]/50',
              )}
            >
              <Icon className="h-4 w-4" aria-hidden />
              {t.label}
            </button>
          );
        })}
      </nav>

      {/* Type filter */}
      <section>
        <h3 className="mb-2 px-3.5 text-[11px] font-semibold uppercase tracking-[0.10em] text-[#52525B]">
          Type
        </h3>
        <div className="flex flex-col gap-px">
          {KINDS.map((k) => {
            const active = kind === k.value;
            return (
              <button
                key={k.value}
                data-testid={`explore-sidebar-kind-${k.value}`}
                onClick={() => setKind(k.value)}
                className={cn(
                  'flex items-center rounded-md border px-3.5 py-2 text-left text-sm transition-colors',
                  active
                    ? 'border-[#00D9FF]/25 bg-[#00D9FF]/[0.08] text-[#00D9FF]'
                    : 'border-transparent text-[#A1A1AA] hover:bg-[#27272A]/50',
                )}
              >
                {k.label}
              </button>
            );
          })}
        </div>
      </section>

      {/* Tags */}
      <section>
        <h3 className="mb-2.5 px-3.5 text-[11px] font-semibold uppercase tracking-[0.10em] text-[#52525B]">
          Tags
        </h3>
        <div className="px-1">
          <TagPillList
            tags={vocab.tags}
            selected={tags}
            onToggle={toggleTag}
            loading={vocab.loading}
          />
        </div>
      </section>
    </aside>
  );
}
