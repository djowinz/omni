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

export const UploadFormSchema = z.object({
  name: z.string().min(1, 'Name is required').max(64, 'Name must be 64 characters or less'),
  bump: z.enum(['patch', 'minor', 'major', 'none']),
  description: z.string().max(500, 'Description must be 500 characters or less').optional(),
  tags: z.array(z.string()).max(10, 'Up to 10 tags').default([]),
  license: z.string().max(64, 'License must be 64 characters or less').optional(),
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
};
