/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { Publish, type PublishProps } from '../steps/publish';

function mkProps(overrides: Partial<PublishProps> = {}): PublishProps {
  return {
    artifactName: 'Full Telemetry',
    state: {
      uploadState: 'uploading',
      progress: null,
      result: null,
      error: null,
    },
    actions: {
      linkAndUpdate: vi.fn(),
      renameAndPublishNew: vi.fn(),
    },
    ...overrides,
  };
}

describe('<Publish>', () => {
  describe('uploading state', () => {
    it('renders spinner, phase + percent, and progress bar fill', () => {
      const props = mkProps({
        state: {
          uploadState: 'uploading',
          progress: { phase: 'sanitize', done: 47, total: 100 },
          result: null,
          error: null,
        },
      });
      render(<Publish {...props} />);

      expect(screen.getByTestId('publish-uploading')).toBeInTheDocument();
      expect(screen.getByTestId('publish-uploading-spinner')).toBeInTheDocument();
      // Phase label maps "sanitize" → "Sanitizing"; percent floors 47/100 → 47%.
      expect(screen.getByTestId('publish-uploading-phase')).toHaveTextContent('Sanitizing · 47%');
      // Bar fill width reflects percent (set inline).
      const fill = screen.getByTestId('publish-uploading-bar-fill') as HTMLElement;
      expect(fill.style.width).toBe('47%');
    });

    it('renders 0% when progress is null (pre-first-frame)', () => {
      const props = mkProps({
        state: {
          uploadState: 'uploading',
          progress: null,
          result: null,
          error: null,
        },
      });
      render(<Publish {...props} />);
      expect(screen.getByTestId('publish-uploading-phase')).toHaveTextContent('0%');
    });
  });

  describe('success state', () => {
    it('renders artifact summary card with Live badge', () => {
      const props = mkProps({
        state: {
          uploadState: 'success',
          progress: null,
          result: {
            artifact_id: 'ov_01J8XKZ9F',
            name: 'Full Telemetry',
            kind: 'bundle',
            tags: ['fps', 'cpu', 'gpu'],
          },
          error: null,
        },
      });
      render(<Publish {...props} />);

      expect(screen.getByTestId('publish-success')).toBeInTheDocument();
      expect(screen.getByText('Successfully Published!')).toBeInTheDocument();

      const card = screen.getByTestId('publish-success-card');
      expect(card).toHaveTextContent('Full Telemetry');
      expect(card).toHaveTextContent('bundle · 3 tags');

      const badge = screen.getByTestId('publish-success-live-badge');
      expect(badge).toHaveTextContent('● Live');
    });

    it('singularises tag count when there is exactly one tag', () => {
      const props = mkProps({
        state: {
          uploadState: 'success',
          progress: null,
          result: {
            artifact_id: 'ov_X',
            name: 'Solo',
            kind: 'overlay',
            tags: ['only'],
          },
          error: null,
        },
      });
      render(<Publish {...props} />);
      expect(screen.getByTestId('publish-success-card')).toHaveTextContent('overlay · 1 tag');
    });
  });

  describe('error state', () => {
    it('renders generic error card with code + detail block for unknown codes', () => {
      const props = mkProps({
        state: {
          uploadState: 'error',
          progress: null,
          result: null,
          error: {
            code: 'RATE_LIMITED',
            message: 'The Omni Hub temporarily rejected your upload. Please try again.',
            detail: 'retry_after 34s',
          },
        },
      });
      render(<Publish {...props} />);

      expect(screen.getByTestId('publish-error')).toBeInTheDocument();
      expect(screen.getByText('Upload Failed')).toBeInTheDocument();
      const detailBlock = screen.getByTestId('publish-error-detail');
      expect(detailBlock).toHaveTextContent('code RATE_LIMITED');
      expect(detailBlock).toHaveTextContent('detail retry_after 34s');

      // Recovery card MUST NOT render for non-AuthorNameConflict errors.
      expect(screen.queryByTestId('publish-recovery-card')).not.toBeInTheDocument();
    });

    it('renders generic error card when AuthorNameConflict has malformed detail', () => {
      const props = mkProps({
        state: {
          uploadState: 'error',
          progress: null,
          result: null,
          error: {
            code: 'AuthorNameConflict',
            message: 'Name already taken under your identity',
            // Not a JSON-stringified AuthorNameConflictDetail — fall back to generic.
            detail: 'not-json',
          },
        },
      });
      render(<Publish {...props} />);
      expect(screen.queryByTestId('publish-recovery-card')).not.toBeInTheDocument();
      expect(screen.getByTestId('publish-error')).toBeInTheDocument();
    });

    it('renders the amber recovery card when error.code === "AuthorNameConflict"', () => {
      const props = mkProps({
        state: {
          uploadState: 'error',
          progress: null,
          result: null,
          error: {
            code: 'AuthorNameConflict',
            message: 'Name already taken under your identity',
            detail: JSON.stringify({
              existing_artifact_id: 'ov_01J8XKZ9FABCDEFG',
              existing_version: '1.3.0',
              last_published_at: '2026-04-18T00:00:00Z',
            }),
          },
        },
      });
      render(<Publish {...props} />);

      expect(screen.getByTestId('publish-recovery-card')).toBeInTheDocument();
      expect(screen.getByText('Name already taken')).toBeInTheDocument();
      // Card surfaces user-typed name, existing artifact id (truncated), and v1.3.0.
      const card = screen.getByTestId('publish-recovery-card');
      expect(card).toHaveTextContent('Full Telemetry');
      expect(card).toHaveTextContent('ov_01J8XKZ9F…');
      expect(card).toHaveTextContent('v1.3.0');
      expect(card).toHaveTextContent('published 2026-04-18');
    });

    it('Link-and-update button label includes the +1 patch bump', () => {
      const props = mkProps({
        state: {
          uploadState: 'error',
          progress: null,
          result: null,
          error: {
            code: 'AuthorNameConflict',
            message: 'Name already taken under your identity',
            detail: JSON.stringify({
              existing_artifact_id: 'ov_X',
              existing_version: '2.5.7',
              last_published_at: '2026-04-18T00:00:00Z',
            }),
          },
        },
      });
      render(<Publish {...props} />);
      expect(screen.getByTestId('publish-recovery-link-and-update')).toHaveTextContent(
        'Link and update → v2.5.8',
      );
    });

    it('Link-and-update click invokes actions.linkAndUpdate(existing_artifact_id)', () => {
      const linkAndUpdate = vi.fn();
      const renameAndPublishNew = vi.fn();
      const existing_artifact_id = 'ov_01J8XKZ9FABCDEFG';
      const props = mkProps({
        actions: { linkAndUpdate, renameAndPublishNew },
        state: {
          uploadState: 'error',
          progress: null,
          result: null,
          error: {
            code: 'AuthorNameConflict',
            message: 'Name already taken under your identity',
            detail: JSON.stringify({
              existing_artifact_id,
              existing_version: '1.3.0',
              last_published_at: '2026-04-18T00:00:00Z',
            }),
          },
        },
      });
      render(<Publish {...props} />);

      fireEvent.click(screen.getByTestId('publish-recovery-link-and-update'));

      expect(linkAndUpdate).toHaveBeenCalledTimes(1);
      expect(linkAndUpdate).toHaveBeenCalledWith(existing_artifact_id);
      expect(renameAndPublishNew).not.toHaveBeenCalled();
    });

    it('Rename-and-publish-new click invokes actions.renameAndPublishNew', () => {
      const linkAndUpdate = vi.fn();
      const renameAndPublishNew = vi.fn();
      const props = mkProps({
        actions: { linkAndUpdate, renameAndPublishNew },
        state: {
          uploadState: 'error',
          progress: null,
          result: null,
          error: {
            code: 'AuthorNameConflict',
            message: 'Name already taken under your identity',
            detail: JSON.stringify({
              existing_artifact_id: 'ov_X',
              existing_version: '1.3.0',
              last_published_at: '2026-04-18T00:00:00Z',
            }),
          },
        },
      });
      render(<Publish {...props} />);

      fireEvent.click(screen.getByTestId('publish-recovery-rename-and-publish-new'));

      expect(renameAndPublishNew).toHaveBeenCalledTimes(1);
      expect(linkAndUpdate).not.toHaveBeenCalled();
    });
  });
});
