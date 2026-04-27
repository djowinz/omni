import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ForkDialog } from '../fork-dialog';

const baseProps = {
  open: true,
  onOpenChange: vi.fn(),
  origin: { name: 'Marathon', author_handle: 'cyberpath#8e2a4c1d' },
  defaultName: "Marathon's HUD (v2)",
  selfHandle: 'starfire#3a7c9f2b',
  existingNames: ['marathon', 'starlight'],
  onFork: vi.fn(),
  onCancel: vi.fn(),
};

describe('ForkDialog', () => {
  it('Discover variant labels source as Remote', () => {
    render(<ForkDialog {...baseProps} sourceKind="remote" />);
    expect(screen.getByText(/From · Remote/)).toBeInTheDocument();
  });

  it('Installed variant labels source as Installed', () => {
    render(<ForkDialog {...baseProps} sourceKind="local" />);
    expect(screen.getByText(/From · Installed/)).toBeInTheDocument();
  });

  it('accepts spaces, punctuation, parens in default name', async () => {
    render(<ForkDialog {...baseProps} sourceKind="remote" />);
    expect(screen.getByRole('button', { name: /^fork$/i })).not.toBeDisabled();
  });

  it('rejects forbidden characters (slash) per fork.rs::sanitize_name', async () => {
    render(<ForkDialog {...baseProps} sourceKind="remote" />);
    const input = screen.getByLabelText(/new workspace name/i);
    await userEvent.clear(input);
    await userEvent.type(input, 'bad/name');
    expect(screen.getByRole('button', { name: /^fork$/i })).toBeDisabled();
    expect(screen.getByText(/forbidden character/i)).toBeInTheDocument();
  });

  it('rejects names that collide with existing workspace overlays (case-insensitive)', async () => {
    render(<ForkDialog {...baseProps} sourceKind="remote" />);
    const input = screen.getByLabelText(/new workspace name/i);
    await userEvent.clear(input);
    await userEvent.type(input, 'MARATHON');
    expect(screen.getByRole('button', { name: /^fork$/i })).toBeDisabled();
    expect(screen.getByText(/already exists/i)).toBeInTheDocument();
  });

  it('rejects Windows reserved stems (CON, aux, NUL.txt)', async () => {
    render(<ForkDialog {...baseProps} sourceKind="remote" />);
    const input = screen.getByLabelText(/new workspace name/i);
    for (const reserved of ['CON', 'aux', 'NUL.txt']) {
      await userEvent.clear(input);
      await userEvent.type(input, reserved);
      expect(screen.getByRole('button', { name: /^fork$/i })).toBeDisabled();
    }
  });

  it('Fork submits onFork({ target_name }) on click', async () => {
    const onFork = vi.fn();
    render(<ForkDialog {...baseProps} sourceKind="remote" onFork={onFork} />);
    await userEvent.click(screen.getByRole('button', { name: /^fork$/i }));
    expect(onFork).toHaveBeenCalledWith({ target_name: "Marathon's HUD (v2)" });
  });
});
