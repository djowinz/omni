import { describe, it, expect } from 'vitest';
import { actionLabelsFor, kebabLabelsFor, type ExploreTab } from '../artifact-actions';

describe('artifact-actions', () => {
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

  it('kebabLabelsFor Discover is empty (no kebab items)', () => {
    expect(kebabLabelsFor('discover')).toEqual([]);
  });

  it('kebabLabelsFor My Uploads is empty (no kebab items)', () => {
    expect(kebabLabelsFor('my-uploads')).toEqual([]);
  });

  it('installed tab has an empty kebab (Check for update item removed in OWI-132; OWI-109 repopulates)', () => {
    expect(kebabLabelsFor('installed')).toEqual([]);
  });

  it('ExploreTab type covers the three sub-tab ids', () => {
    const tabs: ExploreTab[] = ['discover', 'installed', 'my-uploads'];
    expect(tabs).toHaveLength(3);
  });
});
