/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { ArtifactCard } from '../artifact-card';
import { DropdownMenuItem } from '@/components/ui/dropdown-menu';
import type { ArtifactDetail } from '@/lib/share-types';

// ── Fixture ──────────────────────────────────────────────────────────────────
//
// Uses only fields that exist in share-types.ts ArtifactDetail.
// manifest carries description, version, license, and tags so the detail
// variant can surface them. Author label is sourced from
// `author_display_name` per identity-completion-and-display-name spec §6
// (OWI-82) — manifest-author fallback is dead per T1 (manifest does not
// carry author info; the JWS envelope's kid claim is the author identity).
//
const baseFixture: ArtifactDetail = {
  artifact_id: 'art-001',
  kind: 'theme',
  content_hash: 'aabbccdd',
  r2_url: 'https://r2.example.com/art-001.tar.gz',
  thumbnail_url: 'https://cdn.example.com/thumb-001.png',
  author_pubkey: 'deadbeef12345678',
  author_fingerprint_hex: 'a1b2c3d4e5f6',
  author_display_name: 'alice',
  installs: 42,
  reports: 0,
  created_at: 1_700_000_000,
  updated_at: 1_710_000_000,
  status: 'published',
  manifest: {
    name: 'Midnight Blue',
    description: 'A dark blue overlay theme.',
    version: '1.2.3',
    license: 'MIT',
    tags: ['dark', 'blue'],
  },
};

/** Build an ArtifactDetail fixture, allowing per-test overrides. */
function makeArtifact(overrides: Partial<ArtifactDetail> = {}): ArtifactDetail {
  return { ...baseFixture, ...overrides };
}

const fixture = baseFixture;

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('ArtifactCard — grid variant', () => {
  it('renders title, author, and thumbnail', () => {
    render(<ArtifactCard variant="grid" artifact={fixture} />);

    expect(screen.getByTestId('artifact-card-grid')).toBeInTheDocument();
    expect(screen.getByText('Midnight Blue')).toBeInTheDocument();
    // Per identity-completion spec §6: handle is `<display_name>#<8-hex>`
    // where 8-hex is the leading pubkey slice (canonical disambiguator).
    expect(screen.getByText(/by alice#deadbeef/i)).toBeInTheDocument();
    const img = screen.getByRole('img', { name: /midnight blue/i });
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', fixture.thumbnail_url);
  });

  it('shows Installed badge when installed prop is true', () => {
    render(<ArtifactCard variant="grid" artifact={fixture} installed />);

    expect(screen.getByText(/installed/i)).toBeInTheDocument();
  });
});

describe('authorDisplay handle format', () => {
  // Per identity-completion-and-display-name spec §6 (OWI-82). The pubkey
  // slice is ALWAYS rendered (it's the trust anchor — visible at all times);
  // the display_name is the optional friendly prefix.
  const longPubkey = 'eab4d12c0123456789abcdef'.padEnd(64, '0');

  it('renders <display_name>#<8-hex> when name present', () => {
    render(
      <ArtifactCard
        variant="grid"
        artifact={makeArtifact({
          author_pubkey: longPubkey,
          author_display_name: 'starfire',
        })}
      />,
    );
    expect(screen.getByText(/by starfire#eab4d12c/)).toBeInTheDocument();
  });

  it('renders #<8-hex> alone when display_name is null', () => {
    render(
      <ArtifactCard
        variant="grid"
        artifact={makeArtifact({
          author_pubkey: longPubkey,
          author_display_name: null,
        })}
      />,
    );
    expect(screen.getByText(/by #eab4d12c/)).toBeInTheDocument();
  });

  // The whitespace-only input case is covered exclusively by the warn test
  // below (OWI-91): it asserts both the rendering fallback AND the
  // console.warn worker-contract-violation breadcrumb. A separate
  // rendering-only test would re-trigger the warn and emit stderr noise.

  it('console.warn fires when display_name is non-null but trims to empty (OWI-91)', () => {
    // Worker contract violation guard: spec §3.4 requires NFC + trim before
    // persistence, so a non-null display_name that trims to empty is a bug.
    // Surface it via console.warn (don't crash — fall back to slice-only).
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    render(
      <ArtifactCard
        variant="grid"
        artifact={makeArtifact({
          author_pubkey: longPubkey,
          author_display_name: '   ',
        })}
      />,
    );

    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('[authorDisplay]'));
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('eab4d12c'));
    // Still falls back to slice-only — render did not crash.
    expect(screen.getByText(/by #eab4d12c/)).toBeInTheDocument();

    warnSpy.mockRestore();
  });

  it('does NOT warn when display_name is null (no contract violation)', () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    render(
      <ArtifactCard
        variant="grid"
        artifact={makeArtifact({
          author_pubkey: longPubkey,
          author_display_name: null,
        })}
      />,
    );

    expect(warnSpy).not.toHaveBeenCalled();
    warnSpy.mockRestore();
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
