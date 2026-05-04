import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ArtifactCard } from '../artifact-card';
import type { CachedArtifactDetail } from '../../../lib/share-types';

const fixture: CachedArtifactDetail = {
  artifact_id: 'a1',
  content_hash: 'sha256:abc',
  author_pubkey: 'aa'.repeat(32),
  author_fingerprint_hex: 'aabbccdd',
  name: 'Marathon',
  kind: 'theme',
  tags: ['dark', 'gaming'],
  installs: 1234,
  r2_url: '',
  thumbnail_url: '',
  created_at: 0,
  updated_at: 0,
  author_display_name: 'djowinz',
};

describe('ArtifactCard (grid)', () => {
  it('renders the artifact name and author', () => {
    render(<ArtifactCard artifact={fixture} />);
    expect(screen.getByText('Marathon')).toBeInTheDocument();
    expect(screen.getByText(/djowinz/)).toBeInTheDocument();
  });

  it('renders the install count', () => {
    render(<ArtifactCard artifact={fixture} />);
    expect(screen.getByText('1,234')).toBeInTheDocument();
  });

  it('renders the kind badge for theme', () => {
    render(<ArtifactCard artifact={fixture} />);
    expect(screen.getByText('Theme')).toBeInTheDocument();
  });

  it('renders the kind badge for bundle', () => {
    render(<ArtifactCard artifact={{ ...fixture, kind: 'bundle' }} />);
    expect(screen.getByText('Bundle')).toBeInTheDocument();
  });

  it('renders Installed badge when installed prop is true', () => {
    render(<ArtifactCard artifact={fixture} installed />);
    expect(screen.getByText('Installed')).toBeInTheDocument();
  });

  it('fires onClick when the card body is clicked', async () => {
    const onClick = vi.fn();
    render(<ArtifactCard artifact={fixture} onClick={onClick} />);
    await userEvent.click(screen.getByTestId('artifact-card-grid'));
    expect(onClick).toHaveBeenCalled();
  });

  it('renders hover overlay buttons when onPreview/onInstall are provided', () => {
    render(<ArtifactCard artifact={fixture} onPreview={() => {}} onInstall={() => {}} />);
    expect(screen.getByRole('button', { name: /preview/i })).toBeInTheDocument();
    // The Install button is a <button> element (not the card div role="button")
    const installBtn = screen
      .getAllByRole('button', { name: /install/i })
      .find((el) => el.tagName === 'BUTTON');
    expect(installBtn).toBeInTheDocument();
  });

  it('hover Install button does NOT trigger card onClick', async () => {
    const onClick = vi.fn();
    const onInstall = vi.fn();
    render(
      <ArtifactCard
        artifact={fixture}
        onClick={onClick}
        onPreview={() => {}}
        onInstall={onInstall}
      />,
    );
    // Target the <button> element specifically (not the card div role="button")
    const installBtn = screen
      .getAllByRole('button', { name: /install/i })
      .find((el) => el.tagName === 'BUTTON')!;
    await userEvent.click(installBtn);
    expect(onInstall).toHaveBeenCalled();
    expect(onClick).not.toHaveBeenCalled();
  });

  it('hover Preview button does NOT trigger card onClick', async () => {
    const onClick = vi.fn();
    const onPreview = vi.fn();
    render(
      <ArtifactCard
        artifact={fixture}
        onClick={onClick}
        onPreview={onPreview}
        onInstall={() => {}}
      />,
    );
    await userEvent.click(screen.getByRole('button', { name: /preview/i }));
    expect(onPreview).toHaveBeenCalled();
    expect(onClick).not.toHaveBeenCalled();
  });

  it('reflects data-selected via the selection ring class', () => {
    render(<ArtifactCard artifact={fixture} data-selected="true" />);
    expect(screen.getByTestId('artifact-card-grid')).toHaveClass('border-[#00D9FF]');
  });
});
