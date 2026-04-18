/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IdentitySummaryCard } from '../identity-summary-card';

describe('IdentitySummaryCard', () => {
  it('renders pubkey fingerprint + not-backed-up badge', () => {
    render(<IdentitySummaryCard pubkeyHex={'ab'.repeat(32)} fingerprintHex="" backedUp={false} />);
    expect(screen.getByTestId('identity-summary-card')).toBeInTheDocument();
    // With empty fingerprint (#006 deferred), falls back to pubkey short form
    expect(screen.getByText(/abab/)).toBeInTheDocument();
    expect(screen.getByTestId('identity-backup-status')).toHaveTextContent(/not backed up/i);
  });

  it('shows green backed-up badge when backedUp=true', () => {
    render(
      <IdentitySummaryCard pubkeyHex={'bb'.repeat(32)} fingerprintHex="bbbbbbccccdd" backedUp />,
    );
    expect(screen.getByTestId('identity-backup-status')).toHaveTextContent(/backed up/i);
  });
});
