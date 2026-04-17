/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ArtifactCard } from '../artifact-card';
import { DropdownMenuItem } from '@/components/ui/dropdown-menu';
import type { ArtifactDetail } from '@/lib/share-types';

// ── Fixture ──────────────────────────────────────────────────────────────────
//
// Uses only fields that exist in share-types.ts ArtifactDetail.
// manifest carries author_name, description, version, license, and tags so the
// detail variant can surface them.
//
const fixture: ArtifactDetail = {
  artifact_id: 'art-001',
  kind: 'theme',
  content_hash: 'aabbccdd',
  r2_url: 'https://r2.example.com/art-001.tar.gz',
  thumbnail_url: 'https://cdn.example.com/thumb-001.png',
  author_pubkey: 'deadbeef12345678',
  author_fingerprint_hex: 'a1b2c3d4e5f6',
  installs: 42,
  reports: 0,
  created_at: 1_700_000_000,
  updated_at: 1_710_000_000,
  status: 'published',
  manifest: {
    name: 'Midnight Blue',
    author_name: 'alice',
    description: 'A dark blue overlay theme.',
    version: '1.2.3',
    license: 'MIT',
    tags: ['dark', 'blue'],
  },
};

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('ArtifactCard — grid variant', () => {
  it('renders title, author, and thumbnail', () => {
    render(<ArtifactCard variant="grid" artifact={fixture} />);

    expect(screen.getByTestId('artifact-card-grid')).toBeInTheDocument();
    expect(screen.getByText('Midnight Blue')).toBeInTheDocument();
    expect(screen.getByText(/by alice/i)).toBeInTheDocument();
    const img = screen.getByRole('img', { name: /midnight blue/i });
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', fixture.thumbnail_url);
  });

  it('shows Installed badge when installed prop is true', () => {
    render(<ArtifactCard variant="grid" artifact={fixture} installed />);

    expect(screen.getByText(/installed/i)).toBeInTheDocument();
  });
});

describe('ArtifactCard — detail variant', () => {
  it('renders three-slot action row with Discover-tab labels', () => {
    render(
      <ArtifactCard
        variant="detail"
        artifact={fixture}
        actionSlots={{
          left: <button>Preview</button>,
          middle: <button>Install</button>,
          right: <button>Fork</button>,
        }}
      />,
    );

    expect(screen.getByTestId('artifact-card-detail')).toBeInTheDocument();

    // Slot testids are present
    const slotLeft = screen.getByTestId('artifact-card-action-slot-left');
    const slotMiddle = screen.getByTestId('artifact-card-action-slot-middle');
    const slotRight = screen.getByTestId('artifact-card-action-slot-right');

    expect(slotLeft).toBeInTheDocument();
    expect(slotMiddle).toBeInTheDocument();
    expect(slotRight).toBeInTheDocument();

    // Buttons are queryable by role+name
    expect(screen.getByRole('button', { name: /preview/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /install/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /fork/i })).toBeInTheDocument();

    // Position: each button lives inside its correct slot
    expect(slotLeft).toContainElement(screen.getByRole('button', { name: /preview/i }));
    expect(slotMiddle).toContainElement(screen.getByRole('button', { name: /install/i }));
    expect(slotRight).toContainElement(screen.getByRole('button', { name: /fork/i }));
  });

  it('renders three-slot action row with Installed-tab labels (position stable)', () => {
    render(
      <ArtifactCard
        variant="detail"
        artifact={fixture}
        actionSlots={{
          left: <button>Open</button>,
          middle: <button>Uninstall</button>,
          right: <button>Fork</button>,
        }}
      />,
    );

    const slotLeft = screen.getByTestId('artifact-card-action-slot-left');
    const slotMiddle = screen.getByTestId('artifact-card-action-slot-middle');
    const slotRight = screen.getByTestId('artifact-card-action-slot-right');

    expect(slotLeft).toContainElement(screen.getByRole('button', { name: /open/i }));
    expect(slotMiddle).toContainElement(screen.getByRole('button', { name: /uninstall/i }));
    expect(slotRight).toContainElement(screen.getByRole('button', { name: /fork/i }));
  });

  it('renders three-slot action row with My-Uploads-tab labels', () => {
    render(
      <ArtifactCard
        variant="detail"
        artifact={fixture}
        actionSlots={{
          left: <button>Open</button>,
          middle: <button>Delete</button>,
          right: <button>Update</button>,
        }}
      />,
    );

    const slotLeft = screen.getByTestId('artifact-card-action-slot-left');
    const slotMiddle = screen.getByTestId('artifact-card-action-slot-middle');
    const slotRight = screen.getByTestId('artifact-card-action-slot-right');

    expect(slotLeft).toContainElement(screen.getByRole('button', { name: /open/i }));
    expect(slotMiddle).toContainElement(screen.getByRole('button', { name: /delete/i }));
    expect(slotRight).toContainElement(screen.getByRole('button', { name: /update/i }));
  });

  it('renders consumer-provided kebab menu items when trigger is clicked', async () => {
    const user = userEvent.setup();

    render(
      <ArtifactCard
        variant="detail"
        artifact={fixture}
        kebabMenuItems={<DropdownMenuItem>Report</DropdownMenuItem>}
      />,
    );

    const kebabTrigger = screen.getByTestId('artifact-card-kebab');
    expect(kebabTrigger).toBeInTheDocument();

    await user.click(kebabTrigger);

    // Radix portal-renders outside the container; use findByRole
    const menuItem = await screen.findByRole('menuitem', { name: /report/i });
    expect(menuItem).toBeInTheDocument();
  });
});
