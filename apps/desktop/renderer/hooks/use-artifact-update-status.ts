/**
 * Derives "update available" status for installed artifacts.
 *
 * Pure derivation hook — no IPC, no state. Compares each installed registry
 * row's `installed_version` against the worker-side `manifest.version` from
 * the explorer.batchGet payload (already fetched by useInstalledDetails).
 *
 * Returns a Map keyed by artifact_id so the consumer card / detail can do
 * O(1) lookups. Tolerant of malformed semver on either side: bad strings
 * yield `available: false` (never throws, never crashes the tab).
 *
 * Note: the original spec specified `import semver from 'semver'` and
 * `semver.gt()`. The `semver` package is not declared in
 * `apps/desktop/package.json` and adding it is outside this task's
 * file-ownership scope, so we inline an equivalent strict-`>` comparator
 * that handles the same input shapes the spec requires (numeric
 * MAJOR.MINOR.PATCH with optional prerelease/build tags) and treats any
 * unparseable string as "no update".
 */
import { useMemo } from 'react';
import type { InstalledEntryRow, ArtifactDetail } from '../lib/share-types';

export interface UpdateStatus {
  /** True when worker manifest.version > local installed_version. Strict `>`. */
  available: boolean;
  latest_version: string;
  installed_version: string;
}

/**
 * Minimal SemVer 2.0.0 parser. Returns null when the input does not match
 * the `MAJOR.MINOR.PATCH[-prerelease][+build]` grammar (each version
 * component must be a non-negative integer without leading zeros).
 */
interface ParsedSemver {
  major: number;
  minor: number;
  patch: number;
  prerelease: ReadonlyArray<string | number>;
}

// Anchored, no leading zeros on numeric components (per SemVer 2.0.0 §2),
// prerelease and build are optional dotted identifiers.
const SEMVER_RE =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

function parseSemver(input: string): ParsedSemver | null {
  if (typeof input !== 'string') return null;
  const m = SEMVER_RE.exec(input);
  if (!m) return null;
  const [, maj, min, pat, pre] = m;
  const prerelease: Array<string | number> = pre
    ? pre.split('.').map((id) => (/^\d+$/.test(id) ? Number(id) : id))
    : [];
  return {
    major: Number(maj),
    minor: Number(min),
    patch: Number(pat),
    prerelease,
  };
}

/** Compare two prerelease identifier lists per SemVer 2.0.0 §11.4. */
function comparePrerelease(
  a: ReadonlyArray<string | number>,
  b: ReadonlyArray<string | number>,
): number {
  // A version with prerelease has LOWER precedence than one without (§11.3).
  if (a.length === 0 && b.length === 0) return 0;
  if (a.length === 0) return 1; // a (no prerelease) > b (has prerelease)
  if (b.length === 0) return -1;
  const len = Math.min(a.length, b.length);
  for (let i = 0; i < len; i++) {
    const ai = a[i];
    const bi = b[i];
    const aNum = typeof ai === 'number';
    const bNum = typeof bi === 'number';
    if (aNum && bNum) {
      if (ai !== bi) return (ai as number) < (bi as number) ? -1 : 1;
    } else if (aNum) {
      return -1; // numeric < alphanumeric
    } else if (bNum) {
      return 1;
    } else {
      const as = ai as string;
      const bs = bi as string;
      if (as !== bs) return as < bs ? -1 : 1;
    }
  }
  if (a.length !== b.length) return a.length < b.length ? -1 : 1;
  return 0;
}

/** Strict `>` comparison matching `semver.gt(latest, installed)`. */
function semverGt(latest: string, installed: string): boolean {
  const l = parseSemver(latest);
  const i = parseSemver(installed);
  if (!l || !i) {
    // Malformed input on either side — `semver.gt` throws in that case;
    // mirror that by throwing so the caller's catch handles it.
    throw new Error('Invalid semver');
  }
  if (l.major !== i.major) return l.major > i.major;
  if (l.minor !== i.minor) return l.minor > i.minor;
  if (l.patch !== i.patch) return l.patch > i.patch;
  return comparePrerelease(l.prerelease, i.prerelease) > 0;
}

export function useArtifactUpdateStatus(
  entries: InstalledEntryRow[],
  byId: Map<string, ArtifactDetail>,
): Map<string, UpdateStatus> {
  return useMemo(() => {
    const out = new Map<string, UpdateStatus>();
    for (const entry of entries) {
      const detail = byId.get(entry.artifact_id);
      if (!detail) continue;
      const latest = detail.manifest.version;
      const installed = entry.installed_version;
      let available = false;
      try {
        available = semverGt(latest, installed);
      } catch {
        // Malformed semver on either side — treat as "no update". Registry
        // corruption or worker payload drift should not crash the tab.
      }
      out.set(entry.artifact_id, { available, latest_version: latest, installed_version: installed });
    }
    return out;
  }, [entries, byId]);
}
