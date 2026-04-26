/// <reference types="@testing-library/jest-dom/vitest" />
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { UploadDialogFooter, type UploadDialogFooterProps } from '../footer';

function renderFooter(overrides: Partial<UploadDialogFooterProps> = {}) {
  const props: UploadDialogFooterProps = {
    step: 'select',
    state: 'idle',
    primaryDisabled: false,
    onBack: vi.fn(),
    onCancel: vi.fn(),
    onPrimary: vi.fn(),
    ...overrides,
  };
  return { props, ...render(<UploadDialogFooter {...props} />) };
}

describe('UploadDialogFooter', () => {
  describe('Back button visibility', () => {
    it('hides Back on step "select" (INV-7.1.12)', () => {
      renderFooter({ step: 'select' });
      expect(screen.queryByRole('button', { name: /^Back$/ })).toBeNull();
    });

    it('shows Back on step "details"', () => {
      renderFooter({ step: 'details' });
      expect(screen.getByRole('button', { name: /^Back$/ })).toBeInTheDocument();
    });

    it('shows Back on step "packing"', () => {
      renderFooter({ step: 'packing' });
      expect(screen.getByRole('button', { name: /^Back$/ })).toBeInTheDocument();
    });

    it('hides Back while step "upload" is in-flight', () => {
      renderFooter({ step: 'upload', state: 'in-flight' });
      expect(screen.queryByRole('button', { name: /^Back$/ })).toBeNull();
    });

    it('hides Back on step "upload" success', () => {
      renderFooter({ step: 'upload', state: 'success' });
      expect(screen.queryByRole('button', { name: /^Back$/ })).toBeNull();
    });

    it('shows Back on step "upload" error so user can revise', () => {
      renderFooter({ step: 'upload', state: 'error' });
      expect(screen.getByRole('button', { name: /^Back$/ })).toBeInTheDocument();
    });
  });

  describe('Primary CTA label', () => {
    it('renders "Continue ›" on step select', () => {
      renderFooter({ step: 'select' });
      expect(screen.getByRole('button', { name: /Continue/ })).toBeInTheDocument();
    });

    it('renders "Continue ›" on step details', () => {
      renderFooter({ step: 'details' });
      expect(screen.getByRole('button', { name: /Continue/ })).toBeInTheDocument();
    });

    it('renders "Publish ›" on step packing', () => {
      renderFooter({ step: 'packing' });
      expect(screen.getByRole('button', { name: /Publish/ })).toBeInTheDocument();
    });

    it('renders "Done" on step upload + state success', () => {
      renderFooter({ step: 'upload', state: 'success' });
      expect(screen.getByRole('button', { name: /^Done$/ })).toBeInTheDocument();
    });

    it('renders "Retry ›" on step upload + state error', () => {
      renderFooter({ step: 'upload', state: 'error' });
      expect(screen.getByRole('button', { name: /Retry/ })).toBeInTheDocument();
    });
  });

  describe('Primary CTA disabled state', () => {
    it('disables primary button when primaryDisabled=true', () => {
      renderFooter({ primaryDisabled: true });
      const btn = screen.getByRole('button', { name: /Continue/ });
      expect(btn).toBeDisabled();
    });

    it('enables primary button when primaryDisabled=false', () => {
      renderFooter({ primaryDisabled: false });
      const btn = screen.getByRole('button', { name: /Continue/ });
      expect(btn).not.toBeDisabled();
    });
  });

  describe('Click handlers', () => {
    it('fires onBack when Back clicked', () => {
      const { props } = renderFooter({ step: 'details' });
      fireEvent.click(screen.getByRole('button', { name: /^Back$/ }));
      expect(props.onBack).toHaveBeenCalledTimes(1);
    });

    it('fires onCancel when Cancel clicked', () => {
      const { props } = renderFooter({ step: 'details' });
      fireEvent.click(screen.getByRole('button', { name: /^Cancel$/ }));
      expect(props.onCancel).toHaveBeenCalledTimes(1);
    });

    it('fires onPrimary when primary CTA clicked', () => {
      const { props } = renderFooter({ step: 'details' });
      fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
      expect(props.onPrimary).toHaveBeenCalledTimes(1);
    });
  });

  describe('Visual contract', () => {
    it('primary CTA uses cyan #00D9FF background (INV-7.0.3)', () => {
      renderFooter({ step: 'select' });
      const btn = screen.getByRole('button', { name: /Continue/ });
      expect(btn.className).toMatch(/00D9FF/);
    });

    it('renders inside a top-bordered footer row', () => {
      renderFooter({ step: 'select' });
      const footer = screen.getByTestId('upload-dialog-footer');
      expect(footer.className).toMatch(/border-t/);
      expect(footer.className).toMatch(/27272A/);
    });
  });
});
