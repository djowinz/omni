import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { SplashLoader } from '../splash-loader';

describe('SplashLoader', () => {
  it('renders a centered spinner with accessible label', () => {
    render(<SplashLoader />);
    expect(screen.getByRole('status', { name: /loading/i })).toBeTruthy();
  });
});
