import { POLICY_ALLOWED, POLICY_NOT_ALLOWED } from '@/lib/policy';

export interface PolicyDisclosureProps {
  defaultOpen?: boolean;
  className?: string;
}

/**
 * Collapsible two-column content-policy summary. Uses the native `<details>`
 * element — no extra deps. Intended to be embedded in the Upload (#015) and
 * Report (#017) dialogs as a compact read-before-you-publish reference.
 */
export default function PolicyDisclosure({
  defaultOpen = false,
  className,
}: PolicyDisclosureProps) {
  return (
    <details
      open={defaultOpen}
      className={['rounded-md border border-zinc-700/60 bg-zinc-900/60 text-sm', className ?? '']
        .join(' ')
        .trim()}
    >
      <summary
        data-testid="policy-disclosure-toggle"
        className="cursor-pointer select-none px-3 py-2 font-medium text-zinc-300 hover:text-zinc-100"
      >
        Content policy — what you can and can&apos;t publish
      </summary>

      <div className="grid grid-cols-2 gap-4 px-3 pb-3 pt-2">
        {/* Left column — allowed */}
        <div>
          <p className="mb-1.5 font-semibold text-emerald-400">✓ OK to publish</p>
          <ul
            data-testid="policy-disclosure-allowed-list"
            className="list-disc space-y-1 pl-4 text-zinc-400"
          >
            {POLICY_ALLOWED.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </div>

        {/* Right column — not allowed */}
        <div>
          <p className="mb-1.5 font-semibold text-rose-400">✗ Not allowed</p>
          <ul
            data-testid="policy-disclosure-not-allowed-list"
            className="list-disc space-y-1 pl-4 text-zinc-400"
          >
            {POLICY_NOT_ALLOWED.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </div>
      </div>
    </details>
  );
}
