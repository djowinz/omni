import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { InstallProgress } from '../install-progress';

describe('InstallProgress', () => {
  it('renders download phase with 30/100', () => {
    render(<InstallProgress phase="download" done={30} total={100} />);

    const bar = screen.getByTestId('install-progress-bar');
    expect(bar.style.width).toBe('30%');

    // Active pill testid matches current phase
    const pill = screen.getByTestId('install-progress-phase-download');
    expect(pill).toBeTruthy();
    expect(pill.textContent).toContain('download');
  });

  it('renders done state with green bar and all pills showing checkmarks', () => {
    render(<InstallProgress phase="done" done={100} total={100} />);

    const bar = screen.getByTestId('install-progress-bar');
    expect(bar.style.width).toBe('100%');
    expect(bar.className).toContain('bg-emerald-500');

    // All four in-flight phase pills should show ✓
    for (const phase of ['download', 'verify', 'sanitize', 'write']) {
      const pills = screen
        .getAllByText(new RegExp(`✓.*${phase}`))
        .filter((el) => el.tagName.toLowerCase() === 'span');
      expect(pills.length).toBeGreaterThan(0);
    }
  });

  it('renders error state with red bar', () => {
    render(<InstallProgress phase="error" done={45} total={100} />);

    const bar = screen.getByTestId('install-progress-bar');
    expect(bar.className).toContain('bg-red-500');
  });

  it('handles total=0 without divide-by-zero', () => {
    // Should not throw and bar width should be 0%
    render(<InstallProgress phase="download" done={0} total={0} />);

    const root = screen.getByTestId('install-progress');
    expect(root).toBeTruthy();

    const bar = screen.getByTestId('install-progress-bar');
    expect(bar.style.width).toBe('0%');
  });

  it('renders label when provided', () => {
    render(<InstallProgress phase="verify" done={10} total={50} label="Installing theme..." />);

    expect(screen.getByText('Installing theme...')).toBeTruthy();
  });
});
