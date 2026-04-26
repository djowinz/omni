/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * packing.test.tsx — Step 3 Packing UI coverage.
 *
 * Verifies the four PackingStageRow visual states (pending / running / passed
 * / failed), the three Packing summary card branches (all-passed / first
 * failure / aggregate Dependency Check failure), and that the retry button
 * flows back to {@link PackingProps.actions.retry}.
 *
 * Pack progress is mocked at the `useShareWs` boundary by stubbing
 * `window.omni.onShareEvent`. Each test captures the host's frame callback
 * via the stub and synthesizes `upload.packProgress` frames to advance the
 * pipeline. This mirrors the pattern used by `use-share-ws.spec.ts` and keeps
 * the tests free of real WebSocket plumbing.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { fireEvent } from '@testing-library/react';

import { Packing } from '../steps/packing';
import { PackingStageRow } from '../steps/packing-stage-row';
import { PackingViolationsCard } from '../steps/packing-violations-card';
import type { PackingViolation } from '../steps/packing-violations-card';

type FrameCallback = (frame: unknown) => void;

function stubShareEvent(): { fire: FrameCallback; unsubSpy: ReturnType<typeof vi.fn> } {
  let captured: FrameCallback | null = null;
  const unsubSpy = vi.fn();
  vi.stubGlobal('omni', {
    sendShareMessage: vi.fn(),
    onShareEvent: vi.fn().mockImplementation((cb: FrameCallback) => {
      captured = cb;
      return unsubSpy;
    }),
  });
  return {
    fire: (frame: unknown) => {
      if (captured === null) throw new Error('onShareEvent was never subscribed');
      captured(frame);
    },
    unsubSpy,
  };
}

function packFrame(
  stage: 'schema' | 'content-safety' | 'asset' | 'dependency' | 'size',
  status: 'running' | 'passed' | 'failed',
  detail: string | null = null,
) {
  return {
    id: 'pack-1',
    type: 'upload.packProgress',
    params: { stage, status, detail },
  };
}

beforeEach(() => {
  vi.resetModules();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

// ── PackingStageRow — four visual states ────────────────────────────────────

describe('PackingStageRow', () => {
  it('renders the pending state with neutral chrome', () => {
    render(<PackingStageRow title="Schema Validation" subtitle="…" status="pending" />);
    const row = screen.getByTestId('packing-stage-schema-validation');
    expect(row).toHaveAttribute('data-status', 'pending');
    expect(row.className).toMatch(/border-\[#27272A\]/);
  });

  it('renders the running state with the animated pulse marker', () => {
    render(<PackingStageRow title="Asset Verification" subtitle="…" status="running" />);
    const row = screen.getByTestId('packing-stage-asset-verification');
    expect(row).toHaveAttribute('data-status', 'running');
    expect(row.innerHTML).toMatch(/animate-pulse/);
  });

  it('renders the passed state with emerald chrome', () => {
    render(<PackingStageRow title="Schema Validation" subtitle="ok" status="passed" />);
    const row = screen.getByTestId('packing-stage-schema-validation');
    expect(row).toHaveAttribute('data-status', 'passed');
    expect(row.className).toMatch(/border-\[rgba\(16,185,129,0\.6\)\]/);
    expect(row.className).toMatch(/bg-\[rgba\(16,185,129,0\.06\)\]/);
  });

  it('renders the failed state with rose chrome', () => {
    render(<PackingStageRow title="Dependency Check" subtitle="4 violations" status="failed" />);
    const row = screen.getByTestId('packing-stage-dependency-check');
    expect(row).toHaveAttribute('data-status', 'failed');
    expect(row.className).toMatch(/border-\[rgba\(244,63,94,0\.6\)\]/);
    expect(row.className).toMatch(/bg-\[rgba\(244,63,94,0\.06\)\]/);
    expect(row).toHaveTextContent('4 violations');
  });
});

// ── PackingViolationsCard — grouping + retry wiring ─────────────────────────

describe('PackingViolationsCard', () => {
  const violations: PackingViolation[] = [
    { kind: 'unused-file', path: 'images/draft.jpg' },
    { kind: 'unused-file', path: 'images/test-bg.webp' },
    { kind: 'missing-ref', path: 'images/missing.png' },
    { kind: 'content-safety', path: 'images/hud-bg.png', detail: 'flagged · conf 0.91' },
  ];

  it('groups rows by category with category headers and counts', () => {
    render(<PackingViolationsCard violations={violations} onRetry={vi.fn()} />);
    const missing = screen.getByTestId('packing-violations-group-missing-ref');
    expect(missing).toHaveTextContent('Missing files (1)');
    const unused = screen.getByTestId('packing-violations-group-unused-file');
    expect(unused).toHaveTextContent('Unused files (2)');
    const safety = screen.getByTestId('packing-violations-group-content-safety');
    expect(safety).toHaveTextContent('Content-safety (1)');
  });

  it('renders the total violation count in the header', () => {
    render(<PackingViolationsCard violations={violations} onRetry={vi.fn()} />);
    const card = screen.getByTestId('packing-violations-card');
    expect(card).toHaveTextContent('4 dependency violations');
  });

  it('uses kind-default reason copy and respects custom detail overrides', () => {
    render(<PackingViolationsCard violations={violations} onRetry={vi.fn()} />);
    expect(screen.getAllByText('not referenced')).toHaveLength(2);
    expect(screen.getByText('not found')).toBeInTheDocument();
    expect(screen.getByText('flagged · conf 0.91')).toBeInTheDocument();
  });

  it('invokes onRetry when the Retry Verification button is clicked', () => {
    const onRetry = vi.fn();
    render(<PackingViolationsCard violations={violations} onRetry={onRetry} />);
    fireEvent.click(screen.getByTestId('packing-violations-retry'));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});

// ── Packing — summary card branches + stage advancement via packProgress ────

describe('Packing', () => {
  it('starts with all five stages pending and no summary card', () => {
    stubShareEvent();
    render(<Packing actions={{ retry: vi.fn() }} />);

    expect(screen.getByTestId('packing-stage-schema-validation')).toHaveAttribute(
      'data-status',
      'pending',
    );
    expect(screen.getByTestId('packing-stage-content-safety-checks')).toHaveAttribute(
      'data-status',
      'pending',
    );
    expect(screen.getByTestId('packing-stage-asset-verification')).toHaveAttribute(
      'data-status',
      'pending',
    );
    expect(screen.getByTestId('packing-stage-dependency-check')).toHaveAttribute(
      'data-status',
      'pending',
    );
    expect(screen.getByTestId('packing-stage-size-check')).toHaveAttribute(
      'data-status',
      'pending',
    );
    expect(screen.queryByTestId('packing-summary-passed')).toBeNull();
    expect(screen.queryByTestId('packing-summary-failed')).toBeNull();
    expect(screen.queryByTestId('packing-violations-card')).toBeNull();
  });

  it('renders the all-passed summary card after every stage emits passed', () => {
    const { fire } = stubShareEvent();
    render(<Packing actions={{ retry: vi.fn() }} />);

    act(() => {
      fire(packFrame('schema', 'passed'));
      fire(packFrame('content-safety', 'passed'));
      fire(packFrame('asset', 'passed'));
      fire(packFrame('dependency', 'passed'));
      fire(packFrame('size', 'passed'));
    });

    const card = screen.getByTestId('packing-summary-passed');
    expect(card).toHaveTextContent('Verification Complete');
    expect(card).toHaveTextContent('Your overlay has passed all security checks');
  });

  it('renders the first-failure summary card with stage-specific copy + retry', () => {
    const retry = vi.fn();
    const { fire } = stubShareEvent();
    render(<Packing actions={{ retry }} />);

    act(() => {
      fire(packFrame('schema', 'passed'));
      fire(packFrame('content-safety', 'passed'));
      fire(packFrame('asset', 'failed', 'PNG decode failed'));
    });

    const card = screen.getByTestId('packing-summary-failed');
    expect(card).toHaveAttribute('data-failed-stage', 'asset');
    expect(card).toHaveTextContent('Asset Verification Failed');

    fireEvent.click(screen.getByTestId('packing-summary-retry'));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it('renders the aggregate violations card when Dependency Check fails with violations', () => {
    const retry = vi.fn();
    const violations: PackingViolation[] = [
      { kind: 'unused-file', path: 'images/orphan.png' },
      { kind: 'missing-ref', path: 'images/ghost.png' },
    ];
    const { fire } = stubShareEvent();
    render(<Packing actions={{ retry }} violations={violations} />);

    act(() => {
      fire(packFrame('schema', 'passed'));
      fire(packFrame('content-safety', 'passed'));
      fire(packFrame('asset', 'passed'));
      fire(packFrame('dependency', 'failed', '2 violations'));
    });

    // Stage row picks up the violations-count override (INV-7.3.5).
    const depRow = screen.getByTestId('packing-stage-dependency-check');
    expect(depRow).toHaveTextContent('2 violations — see details below');

    const card = screen.getByTestId('packing-violations-card');
    expect(card).toHaveTextContent('2 dependency violations');
    expect(card).toHaveTextContent('images/orphan.png');
    expect(card).toHaveTextContent('images/ghost.png');

    // Generic stage-failure card must NOT render alongside the aggregate card.
    expect(screen.queryByTestId('packing-summary-failed')).toBeNull();

    fireEvent.click(screen.getByTestId('packing-violations-retry'));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it('shows pulse-styled icon on a stage marked running', () => {
    const { fire } = stubShareEvent();
    render(<Packing actions={{ retry: vi.fn() }} />);

    act(() => {
      fire(packFrame('content-safety', 'running', 'scanning…'));
    });

    const row = screen.getByTestId('packing-stage-content-safety-checks');
    expect(row).toHaveAttribute('data-status', 'running');
    expect(row.innerHTML).toMatch(/animate-pulse/);
  });
});
