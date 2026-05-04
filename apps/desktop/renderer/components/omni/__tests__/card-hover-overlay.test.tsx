/// <reference types="@testing-library/jest-dom/vitest" />
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { CardHoverOverlay } from '../card-hover-overlay';

describe('CardHoverOverlay', () => {
  it('renders a Preview button and an Install button', () => {
    render(<CardHoverOverlay onPreview={() => {}} onInstall={() => {}} />);
    expect(screen.getByRole('button', { name: /preview/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /install/i })).toBeInTheDocument();
  });

  it('Preview button stops propagation and calls onPreview', async () => {
    const onPreview = vi.fn();
    const onCard = vi.fn();
    render(
      <div onClick={onCard}>
        <CardHoverOverlay onPreview={onPreview} onInstall={() => {}} />
      </div>,
    );
    await userEvent.click(screen.getByRole('button', { name: /preview/i }));
    expect(onPreview).toHaveBeenCalled();
    expect(onCard).not.toHaveBeenCalled();
  });

  it('Install button stops propagation and calls onInstall', async () => {
    const onInstall = vi.fn();
    const onCard = vi.fn();
    render(
      <div onClick={onCard}>
        <CardHoverOverlay onPreview={() => {}} onInstall={onInstall} />
      </div>,
    );
    await userEvent.click(screen.getByRole('button', { name: /install/i }));
    expect(onInstall).toHaveBeenCalled();
    expect(onCard).not.toHaveBeenCalled();
  });
});
