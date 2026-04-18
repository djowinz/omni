/**
 * ExploreSidebar — 240px filter refinement column.
 *
 * Sections:
 *   1. Kind     — single-select among Theme / Bundle / All (mutually exclusive;
 *                 uses 3 checkboxes so visual weight matches the design mock,
 *                 but checking one unchecks the others — this is a kind
 *                 discriminator, not a multi-select)
 *   2. Sort     — 3-row radio list (New / Installs / Name — the wire enum;
 *                 design calls these Trending/New/Featured/Popular but the
 *                 shipped ExplorerListParamsSchema only defines 3 values)
 *   3. Tags     — checkbox multi-select populated from config.vocab.
 *                 Shows "Loading tags..." while the hook is pending.
 */

import { Checkbox } from '@/components/ui/checkbox';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { Label } from '@/components/ui/label';
import {
  useExploreFilters,
  type ExploreKind,
  type ExploreSort,
} from '../../hooks/use-explore-filters';
import { useConfigVocab } from '../../hooks/use-config-vocab';

const KINDS: { value: ExploreKind; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'theme', label: 'Themes' },
  { value: 'bundle', label: 'Bundles' },
];

const SORTS: { value: ExploreSort; label: string }[] = [
  { value: 'new', label: 'New' },
  { value: 'installs', label: 'Popular' },
  { value: 'name', label: 'A–Z' },
];

export function ExploreSidebar() {
  const { kind, sort, tags, setKind, setSort, setTags } = useExploreFilters();
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
      className="flex w-60 flex-shrink-0 flex-col gap-6 overflow-y-auto border-r border-[#27272A] bg-[#141416] p-4"
    >
      <section>
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-400">
          Kind
        </h3>
        <div className="flex flex-col gap-2">
          {KINDS.map((k) => (
            <div key={k.value} className="flex items-center gap-2">
              <Checkbox
                id={`kind-${k.value}`}
                data-testid={`explore-sidebar-kind-${k.value}`}
                checked={kind === k.value}
                onCheckedChange={() => setKind(k.value)}
              />
              <Label
                htmlFor={`kind-${k.value}`}
                className="cursor-pointer text-sm text-zinc-200"
              >
                {k.label}
              </Label>
            </div>
          ))}
        </div>
      </section>

      <section>
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-400">
          Sort
        </h3>
        <RadioGroup
          value={sort}
          onValueChange={(v) => setSort(v as ExploreSort)}
          className="flex flex-col gap-2"
        >
          {SORTS.map((s) => (
            <div key={s.value} className="flex items-center gap-2">
              <RadioGroupItem
                value={s.value}
                id={`sort-${s.value}`}
                data-testid={`explore-sidebar-sort-${s.value}`}
              />
              <Label
                htmlFor={`sort-${s.value}`}
                className="cursor-pointer text-sm text-zinc-200"
              >
                {s.label}
              </Label>
            </div>
          ))}
        </RadioGroup>
      </section>

      <section>
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-400">
          Tags
        </h3>
        {vocab.loading ? (
          <div className="text-xs text-zinc-500">Loading tags...</div>
        ) : vocab.tags.length === 0 ? (
          <div className="text-xs text-zinc-500">No tags available.</div>
        ) : (
          <div className="flex flex-col gap-2">
            {vocab.tags.map((tag) => (
              <div key={tag} className="flex items-center gap-2">
                <Checkbox
                  id={`tag-${tag}`}
                  data-testid={`explore-sidebar-tag-${tag}`}
                  checked={tags.includes(tag)}
                  onCheckedChange={() => toggleTag(tag)}
                />
                <Label
                  htmlFor={`tag-${tag}`}
                  className="cursor-pointer text-sm text-zinc-200"
                >
                  {tag}
                </Label>
              </div>
            ))}
          </div>
        )}
      </section>
    </aside>
  );
}
