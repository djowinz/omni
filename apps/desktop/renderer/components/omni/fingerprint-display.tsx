import { cn } from '@/lib/utils';

export interface FingerprintDisplayProps {
  /** BIP-39 word triple, pre-computed by the host. */
  words: [string, string, string];
  /** Six distinct emoji, pre-computed by the host. */
  emoji: [string, string, string, string, string, string];
  /** Layout variant. `inline` shows emoji as a hover tooltip; `block` shows emoji on a row below. */
  variant?: 'inline' | 'block';
  /** Whether to surface the emoji at all. Defaults to true. */
  showEmoji?: boolean;
  className?: string;
  /** Optional callback invoked after a successful copy-to-clipboard. */
  onCopy?: () => void;
}

/**
 * Renders an Ed25519 author fingerprint in the user-facing forms produced by
 * the identity crate (BIP-39 triple + 6 distinct emoji). The component is
 * purely presentational — it consumes pre-computed `words`/`emoji` from the
 * host per architecture invariant #11.
 *
 * Clicking the words copies the space-joined word triple to the clipboard
 * (space-joined is friendlier for paste into chat / docs than dot-joined).
 */
export function FingerprintDisplay({
  words,
  emoji,
  variant = 'inline',
  showEmoji = true,
  className,
  onCopy,
}: FingerprintDisplayProps) {
  const wordsJoined = words.join('\u00B7'); // U+00B7 MIDDLE DOT
  const emojiJoined = emoji.join(' ');
  const copyPayload = words.join(' ');

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(copyPayload);
      onCopy?.();
    } catch {
      // Swallow — clipboard can fail in non-secure contexts. Consumers wire
      // their own toast via onCopy; surfacing a failure here would require
      // importing the toast layer and creating a cyclic Wave-4 dependency.
    }
  };

  const wordsButton = (
    <button
      type="button"
      onClick={handleCopy}
      title={variant === 'inline' && showEmoji ? `${emojiJoined}\nClick to copy` : 'Click to copy'}
      aria-label={`Copy fingerprint: ${copyPayload}`}
      className="font-mono text-sm cursor-pointer hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-sm"
    >
      {wordsJoined}
    </button>
  );

  if (variant === 'block') {
    return (
      <div className={cn('flex flex-col gap-1', className)}>
        {wordsButton}
        {showEmoji && (
          <div
            className="font-mono text-base tracking-wide select-none"
            aria-label="Fingerprint emoji"
          >
            {emojiJoined}
          </div>
        )}
      </div>
    );
  }

  return <span className={cn('inline-flex', className)}>{wordsButton}</span>;
}
