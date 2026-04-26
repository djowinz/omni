/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Stepper, type StepperProps } from '../stepper';

describe('Stepper', () => {
  const baseProps: StepperProps = {
    steps: ['Select', 'Details', 'Packing', 'Upload'],
    current: 1, // 0-indexed → "Details" active
    completed: [0],
    error: null,
  };

  it('renders 4 pills in order', () => {
    render(<Stepper {...baseProps} />);
    expect(screen.getByText('Select')).toBeInTheDocument();
    expect(screen.getByText('Details')).toBeInTheDocument();
    expect(screen.getByText('Packing')).toBeInTheDocument();
    expect(screen.getByText('Upload')).toBeInTheDocument();
  });

  it('marks completed step with check glyph', () => {
    render(<Stepper {...baseProps} />);
    expect(screen.getByTestId('stepper-pill-0')).toHaveTextContent('✓');
  });

  it('marks active step with number glyph and cyan styling', () => {
    render(<Stepper {...baseProps} />);
    const active = screen.getByTestId('stepper-pill-1');
    expect(active).toHaveTextContent('2');
    expect(active.className).toMatch(/00D9FF/);
  });

  it('renders error variant when error prop is set', () => {
    render(<Stepper {...baseProps} current={3} error="error" />);
    const errorPill = screen.getByTestId('stepper-pill-3');
    expect(errorPill).toHaveTextContent('!');
    expect(errorPill.className).toMatch(/f43f5e/);
  });

  it('renders warning variant for policy reject', () => {
    render(<Stepper {...baseProps} current={3} error="warning" />);
    const warnPill = screen.getByTestId('stepper-pill-3');
    expect(warnPill).toHaveTextContent('!');
    expect(warnPill.className).toMatch(/f59e0b/);
  });
});
