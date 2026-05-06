/**
 * useOverlayMeta — derive per-overlay metadata that the editor + workspace
 * controls need to enforce the install-vs-fork model.
 *
 * Overlays come from two sources:
 *   1. **User-created** — the user clicked "+ New Overlay" or duplicated/forked
 *      one. The folder lives at `<data_dir>/overlays/<chosen-name>/` and has no
 *      registry entry. The user owns it; full edit rights, plain Delete.
 *   2. **Installed from share explorer** — `<data_dir>/overlays/<artifact_id>/`,
 *      registered in `installed-bundles.json`. The registry row carries
 *      `author_pubkey` (the bundle's signed-author claim).
 *
 * Editability rule: an installed overlay is read-only UNLESS the registry's
 * `author_pubkey` matches the local identity's pubkey. That carves out the
 * round-trip case where a user uploads a bundle, deletes locally, then
 * re-downloads their own — they should still be able to edit it as normal,
 * since they're the actual author.
 *
 * Submenu rule: Delete becomes Uninstall (with confirm dialog + registry
 * removal) when `isInstalled`; otherwise it's plain Delete (folder removal).
 */

import { useMemo } from 'react';
import { useInstalledArtifacts } from './use-installed-artifact-ids';
import { useIdentity } from '../lib/identity-context';

export interface OverlayMeta {
  /** True iff this overlay's folder is in `installed-bundles.json` (i.e.,
   *  came from the share explorer). False for user-created overlays. */
  isInstalled: boolean;
  /** When `isInstalled`, the artifact_id used by `explorer.uninstall`. */
  installedArtifactId?: string;
  /** When `isInstalled`, the registry row's `author_pubkey` (hex). */
  installedAuthorPubkey?: string;
  /** True when the user can edit. False when the overlay is installed AND the
   *  registry's `author_pubkey` doesn't match the local identity (the user
   *  must Fork to make changes). User-created overlays are always editable. */
  editable: boolean;
}

const NON_INSTALLED: OverlayMeta = { isInstalled: false, editable: true };

/** Cross-platform last-segment extractor — registry paths use OS-native
 *  separators (backslashes on Windows). */
function pathBasename(p: string): string {
  const parts = p.split(/[\\/]/).filter((s) => s.length > 0);
  return parts[parts.length - 1] ?? p;
}

/**
 * Returns metadata for the named overlay. `undefined`-name yields the
 * permissive default (used while the active-overlay name is loading).
 */
export function useOverlayMeta(overlayName: string | undefined): OverlayMeta {
  const installed = useInstalledArtifacts();
  const { identity } = useIdentity();

  return useMemo(() => {
    if (!overlayName) return NON_INSTALLED;
    const matching = installed.entries.find(
      (e) => e.kind === 'bundle' && pathBasename(e.installed_path) === overlayName,
    );
    if (!matching) return NON_INSTALLED;
    const editable = identity ? matching.author_pubkey === identity.pubkey_hex : false;
    return {
      isInstalled: true,
      installedArtifactId: matching.artifact_id,
      installedAuthorPubkey: matching.author_pubkey,
      editable,
    };
  }, [overlayName, installed.entries, identity]);
}
