/// <reference types="@testing-library/jest-dom/vitest" />

import { describe, it, expect } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { NuqsTestingAdapter } from 'nuqs/adapters/testing';
import { createElement, type ReactNode } from 'react';
import { useExploreFilters } from '../use-explore-filters';

function makeWrapper(searchParams: string = '') {
  return function Wrapper({ children }: { children: ReactNode }) {
    return createElement(NuqsTestingAdapter, { searchParams, children });
  };
}

describe('useExploreFilters', () => {
  it('defaults: tab=discover, kind=all, sort=new, tags=[], q="", selectedId=null', () => {
    const { result } = renderHook(() => useExploreFilters(), { wrapper: makeWrapper() });
    expect(result.current.tab).toBe('discover');
    expect(result.current.kind).toBe('all');
    expect(result.current.sort).toBe('new');
    expect(result.current.tags).toEqual([]);
    expect(result.current.q).toBe('');
    expect(result.current.selectedId).toBeNull();
  });

  it('parses tab=installed from search params', () => {
    const { result } = renderHook(() => useExploreFilters(), {
      wrapper: makeWrapper('?tab=installed'),
    });
    expect(result.current.tab).toBe('installed');
  });

  it('parses tags as comma-delimited list', () => {
    const { result } = renderHook(() => useExploreFilters(), {
      wrapper: makeWrapper('?tags=dark,gaming'),
    });
    expect(result.current.tags).toEqual(['dark', 'gaming']);
  });

  it('setTab updates state', () => {
    const { result } = renderHook(() => useExploreFilters(), { wrapper: makeWrapper() });
    act(() => {
      result.current.setTab('my-uploads');
    });
    expect(result.current.tab).toBe('my-uploads');
  });

  it('setTags accepts array', () => {
    const { result } = renderHook(() => useExploreFilters(), { wrapper: makeWrapper() });
    act(() => {
      result.current.setTags(['cyberpunk', 'minimal']);
    });
    expect(result.current.tags).toEqual(['cyberpunk', 'minimal']);
  });

  it('setSelectedId round-trips null', () => {
    const { result } = renderHook(() => useExploreFilters(), {
      wrapper: makeWrapper('?a=art-xyz'),
    });
    expect(result.current.selectedId).toBe('art-xyz');
    act(() => {
      result.current.setSelectedId(null);
    });
    expect(result.current.selectedId).toBeNull();
  });

  it('rejects unknown tab values by falling back to default', () => {
    const { result } = renderHook(() => useExploreFilters(), {
      wrapper: makeWrapper('?tab=garbage'),
    });
    expect(result.current.tab).toBe('discover');
  });
});
