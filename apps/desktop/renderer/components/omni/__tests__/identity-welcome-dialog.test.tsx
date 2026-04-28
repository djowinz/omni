import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IdentityWelcomeDialog } from '../identity-welcome-dialog';

describe('IdentityWelcomeDialog', () => {
  it('renders both card-buttons and the disclosure', () => {
    render(<IdentityWelcomeDialog open onSetUpNew={vi.fn()} onImport={vi.fn()} />);
    expect(screen.getByRole('button', { name: /set up a new identity/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /import existing identity/i })).toBeInTheDocument();
    expect(screen.getByText(/already published from another machine/i)).toBeInTheDocument();
  });

  it('Set up new triggers onSetUpNew', async () => {
    const onSetUpNew = vi.fn();
    render(<IdentityWelcomeDialog open onSetUpNew={onSetUpNew} onImport={vi.fn()} />);
    await userEvent.click(screen.getByRole('button', { name: /set up a new identity/i }));
    expect(onSetUpNew).toHaveBeenCalledTimes(1);
  });

  it('Import triggers onImport', async () => {
    const onImport = vi.fn();
    render(<IdentityWelcomeDialog open onSetUpNew={vi.fn()} onImport={onImport} />);
    await userEvent.click(screen.getByRole('button', { name: /import existing identity/i }));
    expect(onImport).toHaveBeenCalledTimes(1);
  });
});
