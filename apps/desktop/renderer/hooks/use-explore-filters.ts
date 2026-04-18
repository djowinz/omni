/**
 * useExploreFilters — URL-backed filter state for the Explore panel.
 *
 * Backs every knob the user can turn inside Explore into query params so
 * back/forward navigation works and deep links (e.g. /home?panel=explore&kind=theme&tags=dark,gaming)
 * are shareable.
 *
 * Query param names:
 *   tab   — 'discover' | 'installed' | 'my-uploads'  (default 'discover')
 *   kind  — 'theme' | 'bundle' | 'all'               (default 'all')
 *   sort  — 'new' | 'installs' | 'name'              (default 'new') — align with ExplorerListParams.sort
 *   tags  — comma-delimited tag slugs                (default [])
 *   q     — free-text search                         (default '')
 *   a     — selected artifact id for the detail pane (default null)
 *
 * Sort values match shipped ExplorerListParamsSchema enum; the design's
 * "Trending / Featured / Popular" labels are UI-only and bind to the same
 * three underlying enum values (per oracle-first discipline).
 */

import {
  useQueryState,
  parseAsString,
  parseAsStringEnum,
  parseAsArrayOf,
} from 'nuqs';

export type ExploreTab = 'discover' | 'installed' | 'my-uploads';
export type ExploreKind = 'theme' | 'bundle' | 'all';
export type ExploreSort = 'new' | 'installs' | 'name';

export interface ExploreFilters {
  tab: ExploreTab;
  kind: ExploreKind;
  sort: ExploreSort;
  tags: string[];
  q: string;
  selectedId: string | null;
  setTab: (next: ExploreTab) => void;
  setKind: (next: ExploreKind) => void;
  setSort: (next: ExploreSort) => void;
  setTags: (next: string[]) => void;
  setQ: (next: string) => void;
  setSelectedId: (next: string | null) => void;
}

export function useExploreFilters(): ExploreFilters {
  const [tab, setTab] = useQueryState(
    'tab',
    parseAsStringEnum<ExploreTab>(['discover', 'installed', 'my-uploads']).withDefault('discover'),
  );
  const [kind, setKind] = useQueryState(
    'kind',
    parseAsStringEnum<ExploreKind>(['theme', 'bundle', 'all']).withDefault('all'),
  );
  const [sort, setSort] = useQueryState(
    'sort',
    parseAsStringEnum<ExploreSort>(['new', 'installs', 'name']).withDefault('new'),
  );
  const [tags, setTags] = useQueryState(
    'tags',
    parseAsArrayOf(parseAsString, ',').withDefault([]),
  );
  const [q, setQ] = useQueryState('q', parseAsString.withDefault(''));
  const [selectedId, setSelectedId] = useQueryState('a', parseAsString);

  return {
    tab,
    kind,
    sort,
    tags,
    q,
    selectedId,
    setTab: (next) => {
      void setTab(next);
    },
    setKind: (next) => {
      void setKind(next);
    },
    setSort: (next) => {
      void setSort(next);
    },
    setTags: (next) => {
      void setTags(next);
    },
    setQ: (next) => {
      void setQ(next);
    },
    setSelectedId: (next) => {
      void setSelectedId(next);
    },
  };
}
