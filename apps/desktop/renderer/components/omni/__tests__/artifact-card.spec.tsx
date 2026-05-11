import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { ArtifactCard } from '../artifact-card';
import type { CachedArtifactDetail } from '@/lib/share-types';

const baseArtifact: CachedArtifactDetail = {
  artifact_id: 'A',
  author_pubkey: '0123456789abcdef',
  author_display_name: 'dev-alice',
  author_fingerprint_hex: 'fp',
  content_hash: 'h',
  created_at: 0,
  updated_at: 0,
  installs: 0,
  kind: 'bundle',
  name: 'HWMon Compact',
  tags: [],
  r2_url: '',
  thumbnail_url: '',
} as CachedArtifactDetail;

describe('<ArtifactCard> update pill', () => {
  it('renders the corner pill when updateStatus.available is true', () => {
    render(
      <ArtifactCard
        artifact={baseArtifact}
        installed
        updateStatus={{ available: true, latest_version: '1.0.1', installed_version: '1.0.0' }}
      />,
    );
    expect(screen.getByTestId('update-pill-corner')).toBeInTheDocument();
  });

  it('does NOT render the corner pill when updateStatus is undefined', () => {
    render(<ArtifactCard artifact={baseArtifact} installed />);
    expect(screen.queryByTestId('update-pill-corner')).not.toBeInTheDocument();
  });

  it('does NOT render the corner pill when updateStatus.available is false', () => {
    render(
      <ArtifactCard
        artifact={baseArtifact}
        installed
        updateStatus={{ available: false, latest_version: '1.0.0', installed_version: '1.0.0' }}
      />,
    );
    expect(screen.queryByTestId('update-pill-corner')).not.toBeInTheDocument();
  });
});
