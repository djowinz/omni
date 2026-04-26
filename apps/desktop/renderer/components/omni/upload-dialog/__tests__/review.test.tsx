/// <reference types="@testing-library/jest-dom/vitest" />
/**
 * Review (Step 2) tests — covers INV-7.2.* (excluding 7.2.4 Preview Image)
 * + INV-7.5.2 (Version Bump on update).
 *
 * Test surface (per plan A1.3 step 7):
 *   1. Name renders with the rose `*`.
 *   2. Description renders WITHOUT a `*`.
 *   3. Version Bump appears only when `state.mode === 'update'`.
 *   4. Tag click toggles selection without firing whole-form validation.
 *   5. License "Custom" reveals the customLicense free-text input.
 *   6. PolicyDisclosure renders the "Read the full policy" link.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';

// Mock useConfigVocab so the Tag badges component renders deterministically
// without spinning up a Share-WS mock for every test.
vi.mock('../../../../hooks/use-config-vocab', () => ({
  useConfigVocab: () => ({
    tags: ['dark', 'light', 'minimal', 'gaming'],
    version: 1,
    loading: false,
    error: null,
    retry: () => {},
  }),
}));

import { Review, type ReviewProps } from '../steps/review';
import {
  DEFAULT_FORM,
  UploadFormSchema,
  type UploadFormValues,
} from '../../../../lib/upload-form-schema';

/**
 * Test harness — instantiates a real react-hook-form bound to the same Zod
 * schema the production dialog uses, then renders <Review/> with whichever
 * mode the test wants. Mirrors how `upload-dialog.tsx` will wire the form.
 */
function Harness({
  mode,
  initial,
}: {
  mode: ReviewProps['state']['mode'];
  initial?: Partial<UploadFormValues>;
}) {
  const form = useForm<UploadFormValues>({
    resolver: zodResolver(UploadFormSchema),
    defaultValues: { ...DEFAULT_FORM, ...initial },
  });
  return <Review state={{ mode }} form={form} />;
}

describe('Review (Step 2)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders Name label with a rose asterisk', () => {
    render(<Harness mode="create" />);
    const nameInput = screen.getByTestId('upload-name');
    // The label is the <label> element associated with the input via htmlFor.
    const label = screen.getByText('Name', { exact: false }).closest('label');
    expect(label).not.toBeNull();
    // The `*` lives inside a <span class="text-[#f43f5e]"> sibling.
    const asterisk = within(label as HTMLElement).getByText('*');
    expect(asterisk).toHaveClass('text-[#f43f5e]');
    // Sanity-check the input is wired up.
    expect(nameInput).toBeInTheDocument();
  });

  it('renders Description label WITHOUT an asterisk', () => {
    render(<Harness mode="create" />);
    const descInput = screen.getByTestId('upload-description');
    const label = screen.getByText('Description', { exact: false }).closest('label');
    expect(label).not.toBeNull();
    // Asserting absence: the label's text is exactly "Description" with no `*` glyph.
    expect((label as HTMLElement).textContent?.trim()).toBe('Description');
    expect(within(label as HTMLElement).queryByText('*')).toBeNull();
    expect(descInput).toBeInTheDocument();
  });

  it('does NOT render Version Bump in create mode', () => {
    render(<Harness mode="create" />);
    expect(screen.queryByTestId('upload-bump')).toBeNull();
    expect(screen.queryByText('Version Bump')).toBeNull();
  });

  it('renders Version Bump in update mode (INV-7.5.2)', () => {
    render(<Harness mode="update" />);
    expect(screen.getByTestId('upload-bump')).toBeInTheDocument();
    expect(screen.getByText('Version Bump')).toBeInTheDocument();
  });

  it('toggles a tag pill on click and reflects the selected state', async () => {
    const user = userEvent.setup();
    render(<Harness mode="create" />);

    const dark = screen.getByTestId('review-tag-badge-dark');
    expect(dark).toHaveAttribute('data-selected', 'false');

    await user.click(dark);
    expect(dark).toHaveAttribute('data-selected', 'true');
    // Selected pill carries the cyan border invariant from INV-7.2.5.
    expect(dark.className).toMatch(/00D9FF/);

    // Toggling again returns to unselected.
    await user.click(dark);
    expect(dark).toHaveAttribute('data-selected', 'false');
  });

  it('does not surface Name-required error on tag toggle (no cascading whole-form validation)', async () => {
    const user = userEvent.setup();
    // Empty Name on purpose — DEFAULT_FORM.name === ''.
    render(<Harness mode="create" />);

    await user.click(screen.getByTestId('review-tag-badge-light'));

    // The Name error message ("Name is required") would render in a
    // <p class="text-xs text-rose-400"> sibling under the Name input. Toggling
    // a tag must NOT trigger whole-form validation, so the message stays absent.
    expect(screen.queryByText('Name is required')).toBeNull();
  });

  it('does NOT show the customLicense input when license is empty/non-Custom', () => {
    render(<Harness mode="create" />);
    // Default (DEFAULT_FORM.license === '') — no custom input visible.
    expect(screen.queryByTestId('review-custom-license-input')).toBeNull();
  });

  it('reveals the customLicense input when License "Custom" is selected', () => {
    // We avoid driving the Radix Select trigger directly because Radix uses
    // pointer-capture APIs jsdom doesn't implement. The form-value path is
    // what the production dialog ends up using via setValue anyway, and that's
    // the wiring we actually care about exercising here.
    render(<Harness mode="create" initial={{ license: 'Custom' }} />);
    expect(screen.getByTestId('review-custom-license-input')).toBeInTheDocument();
  });

  it('renders the PolicyDisclosure with a Read the full policy link', () => {
    render(<Harness mode="create" />);
    const disclosure = screen.getByTestId('review-policy-disclosure');
    expect(disclosure).toBeInTheDocument();

    const link = screen.getByTestId('review-policy-disclosure-link');
    expect(link).toHaveTextContent('Read the full policy');
    expect(link).toHaveAttribute('href');
    expect(link.getAttribute('href')).toMatch(/^https?:\/\//);
  });
});
