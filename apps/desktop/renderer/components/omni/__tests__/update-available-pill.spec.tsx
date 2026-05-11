import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { UpdateAvailablePill } from '../update-available-pill';

const status = { available: true, latest_version: '1.0.1', installed_version: '1.0.0' };

describe('<UpdateAvailablePill>', () => {
  it('renders the latest version in corner variant', () => {
    render(<UpdateAvailablePill status={status} variant="corner" />);
    expect(screen.getByText(/1\.0\.1/)).toBeInTheDocument();
  });

  it('corner variant does not call onClick (non-interactive)', () => {
    const onClick = vi.fn();
    render(<UpdateAvailablePill status={status} variant="corner" onClick={onClick} />);
    fireEvent.click(screen.getByText(/1\.0\.1/));
    expect(onClick).not.toHaveBeenCalled();
  });

  it('header variant fires onClick when clicked', () => {
    const onClick = vi.fn();
    render(<UpdateAvailablePill status={status} variant="header" onClick={onClick} />);
    fireEvent.click(screen.getByRole('button', { name: /update/i }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('header variant renders "Update v{latest_version}"', () => {
    render(<UpdateAvailablePill status={status} variant="header" />);
    expect(screen.getByRole('button', { name: /update v1\.0\.1/i })).toBeInTheDocument();
  });
});
