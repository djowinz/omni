import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TofuMismatchDialog, type TofuFingerprint } from '../tofu-mismatch-dialog';

const previously: TofuFingerprint = {
  display_name: 'cyberpath',
  pubkey_hex: '8e2a4c1d6f0b' + '0'.repeat(52),
  fingerprint_hex: '8e2a4c1d6f0b',
  fingerprint_words: ['apple', 'banana', 'cobra'],
  fingerprint_emoji: ['🦊', '🌲', '🚀', '🧊', '🌙', '⚡'],
};

const incoming: TofuFingerprint = {
  display_name: 'cyberpath',
  pubkey_hex: 'f1b9d7c25a3e' + '0'.repeat(52),
  fingerprint_hex: 'f1b9d7c25a3e',
  fingerprint_words: ['river', 'falcon', 'meadow'],
  fingerprint_emoji: ['🐬', '🍕', '🎸', '🪐', '🌵', '💎'],
};

describe('TofuMismatchDialog', () => {
  it('renders both fingerprint columns with handle, emoji, words, and 8-hex fp line', () => {
    render(
      <TofuMismatchDialog
        open
        onOpenChange={vi.fn()}
        artifactName="marathon"
        previously={previously}
        incoming={incoming}
        onCancel={vi.fn()}
        onTrustNew={vi.fn()}
      />,
    );

    expect(screen.getByText(/author identity changed/i)).toBeInTheDocument();
    expect(screen.getByText(/marathon/)).toBeInTheDocument();
    expect(screen.getByText(/cyberpath#8e2a4c1d/)).toBeInTheDocument();
    expect(screen.getByText(/cyberpath#f1b9d7c2/)).toBeInTheDocument();
    expect(screen.getByText(/apple · banana · cobra/)).toBeInTheDocument();
    expect(screen.getByText(/river · falcon · meadow/)).toBeInTheDocument();
    expect(screen.getByText('fp 8e2a4c1d')).toBeInTheDocument();
    expect(screen.getByText('fp f1b9d7c2')).toBeInTheDocument();
  });

  it('hex display is exactly 8 characters everywhere (never 12)', () => {
    render(
      <TofuMismatchDialog
        open
        onOpenChange={vi.fn()}
        artifactName="marathon"
        previously={previously}
        incoming={incoming}
        onCancel={vi.fn()}
        onTrustNew={vi.fn()}
      />,
    );
    expect(screen.queryByText('8e2a4c1d6f0b')).not.toBeInTheDocument();
    expect(screen.queryByText('f1b9d7c25a3e')).not.toBeInTheDocument();
  });

  it('Cancel triggers onCancel; "Install as new author" triggers onTrustNew', async () => {
    const onCancel = vi.fn();
    const onTrustNew = vi.fn();
    render(
      <TofuMismatchDialog
        open
        onOpenChange={vi.fn()}
        artifactName="marathon"
        previously={previously}
        incoming={incoming}
        onCancel={onCancel}
        onTrustNew={onTrustNew}
      />,
    );
    await userEvent.click(screen.getByRole('button', { name: /^cancel$/i }));
    await userEvent.click(screen.getByRole('button', { name: /install as new author/i }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onTrustNew).toHaveBeenCalledTimes(1);
  });
});
