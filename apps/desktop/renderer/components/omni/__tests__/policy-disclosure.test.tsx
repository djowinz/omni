/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import PolicyDisclosure from '../policy-disclosure';
import { POLICY_ALLOWED, POLICY_NOT_ALLOWED } from '@/lib/policy';

describe('PolicyDisclosure', () => {
  it('renders collapsed by default', () => {
    const { container } = render(<PolicyDisclosure />);
    const details = container.querySelector('details');
    expect(details).not.toBeNull();
    expect(details!.open).toBe(false);
  });

  it('renders open when defaultOpen=true', () => {
    render(<PolicyDisclosure defaultOpen />);
    expect(screen.getByTestId('policy-disclosure-allowed-list')).toBeInTheDocument();
    expect(screen.getByTestId('policy-disclosure-not-allowed-list')).toBeInTheDocument();
  });

  it('renders every POLICY_ALLOWED bullet', () => {
    render(<PolicyDisclosure defaultOpen />);
    for (const bullet of POLICY_ALLOWED) {
      expect(screen.getByText(bullet)).toBeInTheDocument();
    }
  });

  it('renders every POLICY_NOT_ALLOWED bullet', () => {
    render(<PolicyDisclosure defaultOpen />);
    for (const bullet of POLICY_NOT_ALLOWED) {
      expect(screen.getByText(bullet)).toBeInTheDocument();
    }
  });
});
