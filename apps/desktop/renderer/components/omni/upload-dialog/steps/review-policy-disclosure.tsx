/**
 * ReviewPolicyDisclosure — Step 2 footer block reminding the publisher of
 * Omni's content policy. Renders inline copy + a single text link to the
 * full policy doc. Spec INV-7.2.7.
 *
 * The link target is a placeholder constant; the real URL is set by OWI-56
 * (B1.6 — Policy disclosure copy + link). Keeping the constant in this file
 * avoids an out-of-scope edit to a shared link map at A1.3 time.
 */

/**
 * Placeholder policy URL. Replaced by OWI-56 (B1.6) with the real Omni
 * content-policy doc location.
 */
export const POLICY_URL = 'https://omni.example/policy';

export interface ReviewPolicyDisclosureProps {
  /** Optional override — primarily for tests. Defaults to POLICY_URL. */
  policyUrl?: string;
}

export function ReviewPolicyDisclosure({ policyUrl = POLICY_URL }: ReviewPolicyDisclosureProps) {
  return (
    <div data-testid="review-policy-disclosure" className="text-xs leading-relaxed text-zinc-400">
      By publishing, you confirm this content follows Omni&apos;s content policy (no illegal
      content, no harassment, no explicit material).{' '}
      <a
        data-testid="review-policy-disclosure-link"
        href={policyUrl}
        target="_blank"
        rel="noreferrer"
        className="text-[#00D9FF] hover:underline"
      >
        Read the full policy
      </a>
    </div>
  );
}
