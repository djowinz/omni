/// <reference types="@testing-library/jest-dom/vitest" />
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { SortDropdown } from '../sort-dropdown';

describe('SortDropdown', () => {
  it('shows the selected option label on the trigger', () => {
    render(<SortDropdown value="new" onChange={() => {}} />);
    expect(screen.getByRole('combobox')).toHaveTextContent('Newest');
  });

  it('opens and shows all four labels', async () => {
    render(<SortDropdown value="new" onChange={() => {}} />);
    await userEvent.click(screen.getByRole('combobox'));
    expect(screen.getByRole('option', { name: /newest/i })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: /most popular/i })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: /recently updated/i })).toBeInTheDocument();
    expect(screen.getByRole('option', { name: /a–z/i })).toBeInTheDocument();
  });

  it('maps "Most Popular" to enum value "installs"', async () => {
    const onChange = vi.fn();
    render(<SortDropdown value="new" onChange={onChange} />);
    await userEvent.click(screen.getByRole('combobox'));
    await userEvent.click(screen.getByRole('option', { name: /most popular/i }));
    expect(onChange).toHaveBeenCalledWith('installs');
  });

  it('maps "Recently Updated" to enum value "new" (alias)', async () => {
    const onChange = vi.fn();
    render(<SortDropdown value="new" onChange={onChange} />);
    await userEvent.click(screen.getByRole('combobox'));
    await userEvent.click(screen.getByRole('option', { name: /recently updated/i }));
    expect(onChange).toHaveBeenCalledWith('new');
  });

  it('maps "A–Z" to enum value "name"', async () => {
    const onChange = vi.fn();
    render(<SortDropdown value="new" onChange={onChange} />);
    await userEvent.click(screen.getByRole('combobox'));
    await userEvent.click(screen.getByRole('option', { name: /a–z/i }));
    expect(onChange).toHaveBeenCalledWith('name');
  });
});
