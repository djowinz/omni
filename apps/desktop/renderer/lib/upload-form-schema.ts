/**
 * Zod schema for the UploadDialog manifest-review form (#015 step 2).
 *
 * Mirrors the subset of UploadPublishParams that's user-editable. Subset
 * because the dialog fixes `visibility: 'public'` (design §3.2 — no
 * private-share UX in v1), `workspace_path` is set from the source-picker,
 * and `kind` is auto-detected from the workspace entry (theme vs bundle).
 *
 * Field constraints match the worker's publish-route validation so client
 * rejections mirror server rejections (client-side UX sanity, not security).
 */

import { z } from 'zod';

/**
 * SPDX license options surfaced in the Step 2 Review License dropdown.
 *
 * `Custom` is the escape hatch — when picked, the dialog reveals a free-text
 * input below the Select that writes into `customLicense`. Spec INV-7.2.6.
 */
export const LICENSE_OPTIONS = [
  { value: 'MIT', label: 'MIT' },
  { value: 'Apache-2.0', label: 'Apache-2.0' },
  { value: 'GPL-3.0', label: 'GPL-3.0' },
  { value: 'BSD-3-Clause', label: 'BSD-3-Clause' },
  { value: 'CC0-1.0', label: 'CC0-1.0' },
  { value: 'Custom', label: 'Custom' },
] as const;

export type LicenseOption = (typeof LICENSE_OPTIONS)[number]['value'];

export const UploadFormSchema = z.object({
  name: z.string().min(1, 'Name is required').max(64, 'Name must be 64 characters or less'),
  bump: z.enum(['patch', 'minor', 'major', 'none']),
  // Description stays optional (no asterisk in UI) — INV-7.2.3.
  description: z.string().max(500, 'Description must be 500 characters or less').optional(),
  tags: z.array(z.string()).max(10, 'Up to 10 tags').default([]),
  // `license` carries either an SPDX value from LICENSE_OPTIONS OR the literal
  // `'Custom'` sentinel; in the latter case the resolved identifier lives in
  // `customLicense`. Step 2's submit handler is responsible for collapsing the
  // pair back into the single user-visible license string before posting to
  // the worker. INV-7.2.6.
  license: z.string().max(64, 'License must be 64 characters or less').optional(),
  customLicense: z
    .string()
    .max(64, 'Custom license must be 64 characters or less')
    .optional(),
  version: z.string().optional(),
  omni_min_version: z.string().optional(),
});

export type UploadFormValues = z.infer<typeof UploadFormSchema>;

export const BUMP_OPTIONS: { value: UploadFormValues['bump']; label: string }[] = [
  { value: 'patch', label: 'Patch — bug fixes' },
  { value: 'minor', label: 'Minor — new features, backward-compat' },
  { value: 'major', label: 'Major — breaking changes' },
  { value: 'none', label: 'No bump — keep current version' },
];

export const DEFAULT_FORM: UploadFormValues = {
  name: '',
  bump: 'patch',
  description: '',
  tags: [],
  license: '',
  customLicense: '',
  // New publishes default to 1.0.0 — the host's upload.publish handler
  // requires a valid semver and the ReviewStep doesn't expose a version
  // field to the user for new uploads (only bump is relevant on update,
  // which is a separate flow).
  //
  // `omni_min_version` is intentionally NOT defaulted here — the host
  // fills it in from its own `ctx.current_version` (CARGO_PKG_VERSION)
  // when the param is missing, so the authoritative semver is always
  // the running host's own version without round-tripping through
  // display-only env vars.
  version: '1.0.0',
};
