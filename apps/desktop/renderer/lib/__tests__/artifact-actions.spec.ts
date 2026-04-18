import { describe, it, expect } from 'vitest';
import {
  buildShareLink,
  actionLabelsFor,
  kebabLabelsFor,
  type ExploreTab,
} from '../artifact-actions';

describe('artifact-actions', () => {
  it('buildShareLink composes an omni:// deep link', () => {
    expect(buildShareLink('art-abc')).toBe('omni://install?artifact_id=art-abc');
  });

  it('actionLabelsFor Discover returns Preview/Install/Fork', () => {
    expect(actionLabelsFor('discover')).toEqual({
      left: 'Preview',
      middle: 'Install',
      right: 'Fork',
    });
  });

  it('actionLabelsFor Installed returns Open/Uninstall/Fork', () => {
    expect(actionLabelsFor('installed')).toEqual({
      left: 'Open',
      middle: 'Uninstall',
      right: 'Fork',
    });
  });

  it('actionLabelsFor MyUploads returns Open/Delete/Update', () => {
    expect(actionLabelsFor('my-uploads')).toEqual({
      left: 'Open',
      middle: 'Delete',
      right: 'Update',
    });
  });

  it('kebabLabelsFor Discover has Copy ID + Copy share link only', () => {
    expect(kebabLabelsFor('discover')).toEqual(['Copy artifact ID', 'Copy share link']);
  });

  it('kebabLabelsFor Installed adds Check for update', () => {
    expect(kebabLabelsFor('installed')).toEqual([
      'Copy artifact ID',
      'Copy share link',
      'Check for update',
    ]);
  });

  it('buildShareLink rejects empty artifact id at compile-time-adjacent runtime', () => {
    expect(() => buildShareLink('')).toThrow(/artifact_id required/);
  });

  it('ExploreTab type covers the three sub-tab ids', () => {
    const tabs: ExploreTab[] = ['discover', 'installed', 'my-uploads'];
    expect(tabs).toHaveLength(3);
  });
});
