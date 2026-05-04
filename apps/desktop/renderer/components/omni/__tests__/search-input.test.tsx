/// <reference types="@testing-library/jest-dom/vitest" />
import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { SearchInput } from '../search-input';

/** Stateful wrapper so the controlled input accumulates value between keystrokes. */
function ControlledSearchInput({ onChange }: { onChange: (v: string) => void }) {
  const [value, setValue] = useState('');
  return (
    <SearchInput
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

describe('SearchInput', () => {
  it('renders with the provided placeholder and value', () => {
    render(<SearchInput value="" onChange={() => {}} placeholder="Search themes…" />);
    const input = screen.getByRole('searchbox');
    expect(input).toHaveAttribute('placeholder', 'Search themes…');
    expect(input).toHaveValue('');
  });

  it('calls onChange with the new value per keystroke', async () => {
    const onChange = vi.fn();
    render(<ControlledSearchInput onChange={onChange} />);
    const input = screen.getByRole('searchbox');
    await userEvent.type(input, 'foo');
    expect(onChange).toHaveBeenCalledWith('f');
    expect(onChange).toHaveBeenCalledWith('fo');
    expect(onChange).toHaveBeenCalledWith('foo');
  });

  it('reflects the controlled value', () => {
    const { rerender } = render(<SearchInput value="initial" onChange={() => {}} />);
    expect(screen.getByRole('searchbox')).toHaveValue('initial');
    rerender(<SearchInput value="updated" onChange={() => {}} />);
    expect(screen.getByRole('searchbox')).toHaveValue('updated');
  });
});
