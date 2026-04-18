import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { FingerprintDisplay } from '../fingerprint-display';

const WORDS: [string, string, string] = ['alpha', 'bravo', 'charlie'];
const EMOJI: [string, string, string, string, string, string] = [
  '\u{1F31F}', // 🌟
  '\u{1F319}', // 🌙
  '\u{1F525}', // 🔥
  '\u{1F4A7}', // 💧
  '\u{1F308}', // 🌈
  '\u{26A1}', // ⚡
];

describe('FingerprintDisplay', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('inline variant renders words dot-separated in a monospace element', () => {
    render(<FingerprintDisplay words={WORDS} emoji={EMOJI} variant="inline" />);
    const wordsEl = screen.getByText('alpha\u00B7bravo\u00B7charlie');
    expect(wordsEl).toBeInTheDocument();
    expect(wordsEl.className).toContain('font-mono');
  });

  it('block variant renders each emoji in the output', () => {
    render(<FingerprintDisplay words={WORDS} emoji={EMOJI} variant="block" />);
    const emojiRow = screen.getByLabelText('Fingerprint emoji');
    for (const e of EMOJI) {
      expect(emojiRow.textContent).toContain(e);
    }
  });

  it('shows emoji in tooltip when showEmoji + inline', () => {
    render(<FingerprintDisplay words={WORDS} emoji={EMOJI} variant="inline" showEmoji />);
    const btn = screen.getByRole('button');
    const title = btn.getAttribute('title') ?? '';
    for (const e of EMOJI) {
      expect(title).toContain(e);
    }
  });

  it('suppresses emoji in tooltip when showEmoji={false}', () => {
    render(<FingerprintDisplay words={WORDS} emoji={EMOJI} variant="inline" showEmoji={false} />);
    const btn = screen.getByRole('button');
    const title = btn.getAttribute('title') ?? '';
    for (const e of EMOJI) {
      expect(title).not.toContain(e);
    }
  });

  it('fires onCopy after successful clipboard write (space-joined words)', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });

    const onCopy = vi.fn();
    render(<FingerprintDisplay words={WORDS} emoji={EMOJI} onCopy={onCopy} />);

    fireEvent.click(screen.getByRole('button'));
    // Allow the async clipboard promise + onCopy callback to resolve.
    await Promise.resolve();
    await Promise.resolve();

    expect(writeText).toHaveBeenCalledWith('alpha bravo charlie');
    expect(onCopy).toHaveBeenCalledTimes(1);
  });

  it('merges className prop onto the root element', () => {
    const { container } = render(
      <FingerprintDisplay words={WORDS} emoji={EMOJI} variant="inline" className="custom-class" />,
    );
    const root = container.firstElementChild as HTMLElement;
    expect(root.className).toContain('custom-class');
  });
});
