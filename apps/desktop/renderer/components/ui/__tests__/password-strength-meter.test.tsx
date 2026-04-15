import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';

import { PasswordStrengthMeter, computeStrength } from '../password-strength-meter';

describe('computeStrength', () => {
  it('returns "none" for an empty string', () => {
    expect(computeStrength('', 12)).toBe('none');
  });

  it('returns "weak" when the value is shorter than minLength', () => {
    expect(computeStrength('abc', 12)).toBe('weak');
  });

  it('returns "weak" when only a single character class is present, even at minLength', () => {
    expect(computeStrength('abcdefghijkl', 12)).toBe('weak');
  });

  it('returns "medium" when length meets minLength and two classes are present', () => {
    expect(computeStrength('Abcdefghijkl', 12)).toBe('medium');
  });

  it('returns "medium" when all four classes are present but length < minLength + 4', () => {
    // 12 chars, four classes — still medium because length is not >= 16.
    expect(computeStrength('Abcdefgh1!xy', 12)).toBe('medium');
  });

  it('returns "strong" when length >= minLength + 4 and all four classes are present', () => {
    // 16 chars, four classes.
    expect(computeStrength('Abcd1234!@#xyzAB', 12)).toBe('strong');
  });

  it('stays medium when all four classes present but just below the strong length threshold', () => {
    // 13 chars, four classes — still medium (13 < 16).
    expect(computeStrength('Abcd1234!@#xy', 12)).toBe('medium');
  });

  it('respects a custom minLength', () => {
    // 8 chars, three classes, minLength = 8 → medium.
    expect(computeStrength('Abcd1234', 8)).toBe('medium');
    // Same string with minLength = 12 → weak (too short).
    expect(computeStrength('Abcd1234', 12)).toBe('weak');
  });
});

describe('<PasswordStrengthMeter />', () => {
  function getSegments(container: HTMLElement): HTMLElement[] {
    return Array.from(
      container.querySelectorAll<HTMLElement>('[data-slot="password-strength-meter"] > div > div'),
    );
  }

  function isFilled(segment: HTMLElement): boolean {
    // A filled segment carries one of the strength fill classes on top of the base bg-muted.
    return (
      segment.classList.contains('bg-destructive') ||
      segment.classList.contains('bg-yellow-500') ||
      segment.classList.contains('bg-emerald-500')
    );
  }

  it('renders three empty segments and no label text for value=""', () => {
    const { container } = render(<PasswordStrengthMeter value="" />);
    const segments = getSegments(container);
    expect(segments).toHaveLength(3);
    expect(segments.every((s) => !isFilled(s))).toBe(true);
    // Label <span> exists (role=status) but text is empty.
    const status = screen.getByRole('status');
    expect(status.textContent).toBe('');
  });

  it('renders one filled segment and "Weak" label for a weak value', () => {
    const { container } = render(<PasswordStrengthMeter value="abc" />);
    const segments = getSegments(container);
    expect(segments.filter(isFilled)).toHaveLength(1);
    expect(screen.getByText('Weak')).toBeInTheDocument();
  });

  it('renders two filled segments and "Medium" label for a medium value', () => {
    const { container } = render(<PasswordStrengthMeter value="Abcdefghijkl" />);
    const segments = getSegments(container);
    expect(segments.filter(isFilled)).toHaveLength(2);
    expect(screen.getByText('Medium')).toBeInTheDocument();
  });

  it('renders three filled segments and "Strong" label for a strong value', () => {
    const { container } = render(<PasswordStrengthMeter value="Abcd1234!@#xyzAB" />);
    const segments = getSegments(container);
    expect(segments.filter(isFilled)).toHaveLength(3);
    expect(screen.getByText('Strong')).toBeInTheDocument();
  });

  it('merges a custom className into the root element', () => {
    const { container } = render(
      <PasswordStrengthMeter value="" className="custom-cls another-cls" />,
    );
    const root = container.querySelector('[data-slot="password-strength-meter"]');
    expect(root).toHaveClass('custom-cls');
    expect(root).toHaveClass('another-cls');
    // Base classes from the component are still present.
    expect(root).toHaveClass('flex');
    expect(root).toHaveClass('flex-col');
  });

  it('respects a custom minLength prop (8-char value becomes medium instead of weak)', () => {
    const { rerender, container } = render(
      <PasswordStrengthMeter value="Abcd1234" minLength={12} />,
    );
    expect(screen.getByText('Weak')).toBeInTheDocument();
    expect(getSegments(container).filter(isFilled)).toHaveLength(1);

    rerender(<PasswordStrengthMeter value="Abcd1234" minLength={8} />);
    expect(screen.getByText('Medium')).toBeInTheDocument();
    expect(getSegments(container).filter(isFilled)).toHaveLength(2);
  });

  it('uses a default minLength of 12 when the prop is omitted', () => {
    // 12 chars, two classes → medium under default minLength = 12.
    render(<PasswordStrengthMeter value="Abcdefghijkl" />);
    expect(screen.getByText('Medium')).toBeInTheDocument();
  });
});
