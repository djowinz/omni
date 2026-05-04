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

/**
 * Kebab menu item labels per tab.
 *
 * Discover + My-Uploads have no kebab items (the previous "Copy artifact ID"
 * + "Copy share link" entries were removed: artifact_id is an internal
 * identifier with no end-user use case, and the share link is an omni://
 * deep-link whose protocol handler isn't shipped yet — both were UI bloat).
 *
 * Installed keeps "Check for update" because it's a meaningful per-row
 * action that doesn't fit the three-slot footer.
 */
export function kebabLabelsFor(tab: ExploreTab): string[] {
  if (tab === 'installed') {
    return ['Check for update'];
  }
  return [];
}
