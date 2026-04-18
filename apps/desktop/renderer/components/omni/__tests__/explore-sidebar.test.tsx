/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { NuqsTestingAdapter } from 'nuqs/adapters/testing';

function Wrap({ children, sp = '' }: { children: ReactNode; sp?: string }) {
  return <NuqsTestingAdapter searchParams={sp}>{children}</NuqsTestingAdapter>;
}

const defaultVocab = {
  tags: ['dark', 'minimal', 'gaming'],
  version: 1,
  loading: false,
  error: null,
};

describe('ExploreSidebar', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('renders Kind / Sort / Tags sections', async () => {
    vi.doMock('../../../hooks/use-config-vocab', () => ({
      useConfigVocab: () => defaultVocab,
    }));
    const { ExploreSidebar } = await import('../explore-sidebar');
    render(
      <Wrap>
        <ExploreSidebar />
      </Wrap>,
    );
    expect(screen.getByTestId('explore-sidebar')).toBeInTheDocument();
    expect(screen.getByText(/Kind/i)).toBeInTheDocument();
    expect(screen.getByText(/Sort/i)).toBeInTheDocument();
    expect(screen.getByText(/Tags/i)).toBeInTheDocument();
  });

  it('renders the 3 vocab tags from useConfigVocab', async () => {
    vi.doMock('../../../hooks/use-config-vocab', () => ({
      useConfigVocab: () => defaultVocab,
    }));
    const { ExploreSidebar } = await import('../explore-sidebar');
    render(
      <Wrap>
        <ExploreSidebar />
      </Wrap>,
    );
    expect(screen.getByText('dark')).toBeInTheDocument();
    expect(screen.getByText('minimal')).toBeInTheDocument();
    expect(screen.getByText('gaming')).toBeInTheDocument();
  });

  it('clicking a Kind checkbox toggles it (visible in the checkbox data-state)', async () => {
    vi.doMock('../../../hooks/use-config-vocab', () => ({
      useConfigVocab: () => defaultVocab,
    }));
    const user = userEvent.setup();
    const { ExploreSidebar } = await import('../explore-sidebar');
    render(
      <Wrap>
        <ExploreSidebar />
      </Wrap>,
    );
    const themeCheckbox = screen.getByTestId('explore-sidebar-kind-theme');
    await user.click(themeCheckbox);
    expect(themeCheckbox.getAttribute('data-state')).toBe('checked');
  });

  it('renders empty state while vocab loads', async () => {
    vi.doMock('../../../hooks/use-config-vocab', () => ({
      useConfigVocab: () => ({ tags: [], version: null, loading: true, error: null }),
    }));
    const { ExploreSidebar } = await import('../explore-sidebar');
    render(
      <Wrap>
        <ExploreSidebar />
      </Wrap>,
    );
    expect(screen.getByText(/Loading tags/i)).toBeInTheDocument();
  });
});
