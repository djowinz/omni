import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { TagPillList } from '../tag-pill-list';

describe('TagPillList', () => {
  it('renders a pill button per tag', () => {
    render(
      <TagPillList tags={['dark', 'light', 'gaming']} selected={[]} onToggle={() => {}} />,
    );
    expect(screen.getByRole('button', { name: 'dark' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'light' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'gaming' })).toBeInTheDocument();
  });

  it('marks selected pills with aria-pressed=true', () => {
    render(
      <TagPillList tags={['dark', 'light']} selected={['dark']} onToggle={() => {}} />,
    );
    expect(screen.getByRole('button', { name: 'dark' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'light' })).toHaveAttribute('aria-pressed', 'false');
  });

  it('calls onToggle with the clicked tag', async () => {
    const onToggle = vi.fn();
    render(<TagPillList tags={['dark']} selected={[]} onToggle={onToggle} />);
    await userEvent.click(screen.getByRole('button', { name: 'dark' }));
    expect(onToggle).toHaveBeenCalledWith('dark');
  });

  it('renders 6 skeleton pills when loading', () => {
    render(<TagPillList tags={[]} selected={[]} onToggle={() => {}} loading />);
    expect(screen.getAllByTestId('tag-pill-skeleton')).toHaveLength(6);
  });

  it('renders empty-vocabulary message when not loading and tags is empty', () => {
    render(<TagPillList tags={[]} selected={[]} onToggle={() => {}} />);
    expect(screen.getByText(/no tags available/i)).toBeInTheDocument();
  });
});
