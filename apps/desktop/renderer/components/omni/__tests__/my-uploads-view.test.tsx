/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { NuqsTestingAdapter } from 'nuqs/adapters/testing';
import type { CachedArtifactDetail } from '../../../lib/share-types';

const FIXTURE: CachedArtifactDetail = {
  artifact_id: 'mine-1',
  content_hash: 'h',
  author_pubkey: 'cc'.repeat(32),
  author_fingerprint_hex: '',
  name: 'My Theme',
  kind: 'theme',
  tags: [],
  installs: 0,
  r2_url: 'https://x/a',
  thumbnail_url: 'https://x/t',
  created_at: 0,
  updated_at: 1700000000,
};

function Wrap({ children }: { children: ReactNode }) {
  return <NuqsTestingAdapter searchParams="">{children}</NuqsTestingAdapter>;
}

describe('MyUploadsView', () => {
  beforeEach(() => {
    vi.resetModules();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders identity card + grid when identity present + items returned', async () => {
    vi.doMock('../../../hooks/use-my-uploads', () => ({
      useMyUploads: () => ({
        items: [FIXTURE],
        nextCursor: null,
        loading: false,
        error: null,
        loadMore: vi.fn(),
        refetch: vi.fn(),
        identityPubkey: 'cc'.repeat(32),
      }),
    }));
    const { MyUploadsView } = await import('../my-uploads-view');

    render(
      <Wrap>
        <MyUploadsView />
      </Wrap>,
    );

    expect(screen.getByTestId('identity-summary-card')).toBeInTheDocument();
    expect(screen.getByText('My Theme')).toBeInTheDocument();
  });

  it('renders empty state when items list is empty', async () => {
    vi.doMock('../../../hooks/use-my-uploads', () => ({
      useMyUploads: () => ({
        items: [],
        nextCursor: null,
        loading: false,
        error: null,
        loadMore: vi.fn(),
        refetch: vi.fn(),
        identityPubkey: 'cc'.repeat(32),
      }),
    }));
    const { MyUploadsView } = await import('../my-uploads-view');

    render(
      <Wrap>
        <MyUploadsView />
      </Wrap>,
    );

    expect(screen.getByText(/haven't published anything yet/i)).toBeInTheDocument();
  });
});
