import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { UpdateConfirmDialog } from '../update-confirm-dialog';
import type { InstalledEntryRow, ArtifactDetail } from '@/lib/share-types';

// Fields per renderer-facing InstalledEntrySchema in share-types.ts.
const installed: InstalledEntryRow = {
  artifact_id: 'A',
  name: 'HWMon Compact',
  kind: 'bundle',
  content_hash: 'h',
  author_pubkey: 'pk-original',
  author_fingerprint_hex: 'fp',
  installed_version: '1.0.0',
  installed_path: '',
  installed_at: 0,
};

// Double-cast through unknown because ArtifactDetail's full shape carries fields
// (e.g. manifest typing) that aren't worth replicating in a behavioural test.
const artifact: ArtifactDetail = {
  artifact_id: 'A',
  author_pubkey: 'pk-original',
  author_display_name: 'dev-alice',
  author_fingerprint_hex: 'fp',
  content_hash: 'h2',
  created_at: 0,
  updated_at: Math.floor(Date.now() / 1000),
  installs: 0,
  kind: 'bundle',
  manifest: {
    name: 'HWMon Compact',
    description: '',
    tags: [],
    license: '',
    version: '1.0.1',
    omni_min_version: '0.1.0',
  },
  r2_url: '',
  reports: 0,
  status: 'live',
  thumbnail_url: '',
} as unknown as ArtifactDetail;

// `send` is the only external dependency we mock — the dialog component should
// import it from useShareWs (or accept it as a prop for testability).
vi.mock('@/hooks/use-share-ws', () => ({
  useShareWs: () => ({ send: vi.fn().mockResolvedValue({}) }),
}));

describe('<UpdateConfirmDialog>', () => {
  it('renders installed and latest version strings', () => {
    render(
      <UpdateConfirmDialog
        open
        onOpenChange={() => {}}
        artifact={artifact}
        installed={installed}
        onApplied={() => {}}
      />,
    );
    expect(screen.getByText(/v1\.0\.0/)).toBeInTheDocument();
    expect(screen.getByText(/v1\.0\.1/)).toBeInTheDocument();
  });

  it('renders the author display name', () => {
    render(
      <UpdateConfirmDialog
        open
        onOpenChange={() => {}}
        artifact={artifact}
        installed={installed}
        onApplied={() => {}}
      />,
    );
    expect(screen.getByText(/dev-alice/)).toBeInTheDocument();
  });

  it('does NOT show the pubkey-rotation pre-warning when keys match', () => {
    render(
      <UpdateConfirmDialog
        open
        onOpenChange={() => {}}
        artifact={artifact}
        installed={installed}
        onApplied={() => {}}
      />,
    );
    expect(screen.queryByText(/Author key changed/i)).not.toBeInTheDocument();
  });

  it('shows the pubkey-rotation pre-warning when keys differ', () => {
    const rotated: ArtifactDetail = { ...artifact, author_pubkey: 'pk-rotated' };
    render(
      <UpdateConfirmDialog
        open
        onOpenChange={() => {}}
        artifact={rotated}
        installed={installed}
        onApplied={() => {}}
      />,
    );
    expect(screen.getByText(/Author key changed/i)).toBeInTheDocument();
  });

  it('Cancel closes the dialog without firing onApplied', () => {
    const onOpenChange = vi.fn();
    const onApplied = vi.fn();
    render(
      <UpdateConfirmDialog
        open
        onOpenChange={onOpenChange}
        artifact={artifact}
        installed={installed}
        onApplied={onApplied}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onApplied).not.toHaveBeenCalled();
  });

  it('Apply closes the dialog', () => {
    const onOpenChange = vi.fn();
    render(
      <UpdateConfirmDialog
        open
        onOpenChange={onOpenChange}
        artifact={artifact}
        installed={installed}
        onApplied={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: /^apply/i }));
    // setTimeout flush — onOpenChange(false) is invoked after the send promise resolves.
    return Promise.resolve().then(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });
});
