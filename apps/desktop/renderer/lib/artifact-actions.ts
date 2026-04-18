/**
 * Pure helpers shared across Explore sub-tabs. No React, no state.
 *
 * Action labels are per-tab stable (design doc §1.3 — three-slot layout):
 *   left slot   (passive view)      = Preview / Open / Open
 *   middle slot (state toggle)      = Install / Uninstall / Delete
 *   right slot  (derivative action) = Fork / Fork / Update
 *
 * Real click handlers wire up in <explore-detail>; this module only
 * returns label strings so the three positions stay visually stable as
 * the user flips between tabs.
 */

export type ExploreTab = 'discover' | 'installed' | 'my-uploads';

export interface ActionLabels {
  left: string;
  middle: string;
  right: string;
}

export function actionLabelsFor(tab: ExploreTab): ActionLabels {
  switch (tab) {
    case 'discover':
      return { left: 'Preview', middle: 'Install', right: 'Fork' };
    case 'installed':
      return { left: 'Open', middle: 'Uninstall', right: 'Fork' };
    case 'my-uploads':
      return { left: 'Open', middle: 'Delete', right: 'Update' };
  }
}

/** Kebab menu item label order per tab (design §5.1). */
export function kebabLabelsFor(tab: ExploreTab): string[] {
  const base = ['Copy artifact ID', 'Copy share link'];
  if (tab === 'installed') {
    return [...base, 'Check for update'];
  }
  return base;
}

/** omni:// deep-link format. Actual protocol handler ships in sub-spec #018; we only build the string. */
export function buildShareLink(artifactId: string): string {
  if (artifactId.length === 0) {
    throw new Error('artifact_id required');
  }
  return `omni://install?artifact_id=${artifactId}`;
}
