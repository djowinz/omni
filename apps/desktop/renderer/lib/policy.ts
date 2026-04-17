/**
 * Content-policy bullet lists — single source of truth for the Upload (#015)
 * and Report (#017) dialogs.
 *
 * Voice: moderator-level, hobbyist scope. Keep entries short enough to scan
 * at a glance; full policy prose lives in the docs site.
 */

export const POLICY_ALLOWED: readonly string[] = [
  'Original overlays, themes, and bundles you created',
  'Forks and remixes that credit the original author in the description',
  'Mature-but-not-sexual themes (dark humor, horror aesthetics)',
  'Themes using public-domain or properly licensed imagery and fonts',
  'Bundles with sensor mappings for any hardware you have data for',
];

export const POLICY_NOT_ALLOWED: readonly string[] = [
  'Sexually explicit or nudity-focused content',
  'Content that impersonates another creator or their distinctive style',
  'Illegal content — piracy, extremism, or harassment campaigns',
  'Malware, exploits, or content designed to crash the overlay',
  'Third-party trademarks or logos without clear parody or commentary framing',
];
