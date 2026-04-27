import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { InlineError } from '../inline-error';

describe('InlineError', () => {
  it('renders the message and triggers onRetry on retry click', async () => {
    const onRetry = vi.fn();
    render(<InlineError message="Sanitize failed: bad font" onRetry={onRetry} />);

    expect(screen.getByText(/sanitize failed: bad font/i)).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: /retry/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('renders the report button when onReport is provided', async () => {
    const onReport = vi.fn();
    render(<InlineError message="x" onRetry={vi.fn()} onReport={onReport} />);
    await userEvent.click(screen.getByRole('button', { name: /report issue/i }));
    expect(onReport).toHaveBeenCalledTimes(1);
  });

  it('omits the report button when onReport is undefined', () => {
    render(<InlineError message="x" onRetry={vi.fn()} />);
    expect(screen.queryByRole('button', { name: /report issue/i })).toBeNull();
  });
});
