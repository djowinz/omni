/**
 * UploadDialog step renderers. Four steps, kept here so the main dialog
 * file stays focused on state-machine + form plumbing.
 *
 *   1. SourceStep    — pick workspace item to publish (skipped if prefilled)
 *   2. ReviewStep    — edit manifest fields (react-hook-form)
 *   3. ValidateStep  — upload.pack dry-run; show sanitize report + sizes
 *   4. PublishStep   — upload.publish with live progress; on success show
 *                      artifact_id + deep link + status (created/deduplicated)
 */

import { useConfigVocab } from '../../hooks/use-config-vocab';
import { InstallProgress } from './install-progress';
import PolicyDisclosure from './policy-disclosure';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Label } from '@/components/ui/label';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { BUMP_OPTIONS, type UploadFormValues } from '../../lib/upload-form-schema';
import type { UseFormReturn } from 'react-hook-form';
import type {
  UploadPackResult,
  UploadPublishProgress,
  UploadPublishResult,
  ShareWsError,
} from '../../lib/share-types';

// ── Step 1 — Source picker ──────────────────────────────────────────────

export interface SourceStepProps {
  overlays: string[];
  themes: string[];
  loading: boolean;
  selected: string | null;
  onSelect: (workspace_path: string) => void;
}

export function SourceStep({ overlays, themes, loading, selected, onSelect }: SourceStepProps) {
  if (loading) {
    return <p className="text-sm text-zinc-500">Loading workspace…</p>;
  }
  if (overlays.length === 0 && themes.length === 0) {
    return (
      <p className="text-sm text-zinc-400">
        No overlays or themes found. Create something in the Components panel first.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-4">
      {overlays.length > 0 && (
        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-400">
            Overlays
          </h3>
          <div className="flex flex-col gap-1">
            {overlays.map((name) => {
              const path = `overlays/${name}`;
              return (
                <button
                  key={path}
                  type="button"
                  data-testid={`upload-source-overlay-${name}`}
                  onClick={() => onSelect(path)}
                  className={
                    selected === path
                      ? 'rounded-md border border-[#00D9FF] bg-[#00D9FF]/10 px-3 py-2 text-left text-sm text-[#FAFAFA]'
                      : 'rounded-md border border-[#27272A] px-3 py-2 text-left text-sm text-zinc-300 hover:bg-[#27272A]'
                  }
                >
                  {name}
                </button>
              );
            })}
          </div>
        </section>
      )}
      {themes.length > 0 && (
        <section>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-zinc-400">
            Themes
          </h3>
          <div className="flex flex-col gap-1">
            {themes.map((file) => {
              const path = `themes/${file}`;
              return (
                <button
                  key={path}
                  type="button"
                  data-testid={`upload-source-theme-${file}`}
                  onClick={() => onSelect(path)}
                  className={
                    selected === path
                      ? 'rounded-md border border-[#00D9FF] bg-[#00D9FF]/10 px-3 py-2 text-left text-sm text-[#FAFAFA]'
                      : 'rounded-md border border-[#27272A] px-3 py-2 text-left text-sm text-zinc-300 hover:bg-[#27272A]'
                  }
                >
                  {file}
                </button>
              );
            })}
          </div>
        </section>
      )}
    </div>
  );
}

// ── Step 2 — Review manifest ────────────────────────────────────────────

export interface ReviewStepProps {
  form: UseFormReturn<UploadFormValues>;
}

export function ReviewStep({ form }: ReviewStepProps) {
  const vocab = useConfigVocab();
  const {
    register,
    formState: { errors },
    setValue,
    watch,
  } = form;
  const selectedTags = watch('tags');
  const bump = watch('bump');

  const toggleTag = (tag: string) => {
    const next = selectedTags.includes(tag)
      ? selectedTags.filter((t) => t !== tag)
      : [...selectedTags, tag];
    // Don't cascade whole-form validation on tag toggles — the step's
    // Advance button already calls form.trigger() before moving on. Passing
    // shouldValidate: true here fired Name-required errors before the user
    // even reached the Name field.
    setValue('tags', next, { shouldValidate: false, shouldDirty: true });
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <Label htmlFor="upload-name">Name</Label>
        <Input id="upload-name" {...register('name')} data-testid="upload-name" />
        {errors.name ? <p className="text-xs text-rose-400">{errors.name.message}</p> : null}
      </div>

      <div className="flex flex-col gap-1">
        <Label>Version bump</Label>
        <Select
          value={bump}
          onValueChange={(v) =>
            // Same reasoning as toggleTag — defer whole-form validation to
            // the step's Advance handler (form.trigger()).
            setValue('bump', v as UploadFormValues['bump'], {
              shouldValidate: false,
              shouldDirty: true,
            })
          }
        >
          <SelectTrigger data-testid="upload-bump">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {BUMP_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="flex flex-col gap-1">
        <Label htmlFor="upload-description">Description (optional)</Label>
        <Textarea
          id="upload-description"
          rows={3}
          {...register('description')}
          data-testid="upload-description"
        />
        {errors.description ? (
          <p className="text-xs text-rose-400">{errors.description.message}</p>
        ) : null}
      </div>

      <div className="flex flex-col gap-1">
        <Label>Tags</Label>
        {vocab.loading ? (
          <p className="text-xs text-zinc-500">Loading tags…</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {vocab.tags.map((tag) => (
              <div key={tag} className="flex items-center gap-1.5">
                <Checkbox
                  id={`upload-tag-${tag}`}
                  data-testid={`upload-tag-${tag}`}
                  checked={selectedTags.includes(tag)}
                  onCheckedChange={() => toggleTag(tag)}
                />
                <Label htmlFor={`upload-tag-${tag}`} className="cursor-pointer text-sm">
                  {tag}
                </Label>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="flex flex-col gap-1">
        <Label htmlFor="upload-license">License (optional)</Label>
        <Input id="upload-license" {...register('license')} data-testid="upload-license" />
      </div>

      <PolicyDisclosure />
    </div>
  );
}

// ── Step 3 — Validate (pack dry-run) ─────────────────────────────────────

export interface ValidateStepProps {
  packing: boolean;
  result: UploadPackResult | null;
  error: ShareWsError | null;
}

export function ValidateStep({ packing, result, error }: ValidateStepProps) {
  if (packing) {
    return (
      <div className="flex items-center gap-2 text-sm text-zinc-400">
        <span className="h-2 w-2 animate-pulse rounded-full bg-[#00D9FF]" />
        Packing…
      </div>
    );
  }
  if (error) {
    return (
      <div
        data-testid="upload-validate-error"
        className="flex flex-col gap-2 rounded-md border border-rose-800 bg-rose-950/30 p-3 text-sm text-rose-200"
      >
        <span className="font-medium">{error.message}</span>
        {typeof error.detail === 'string' && error.detail.length > 0 ? (
          <pre className="max-h-40 overflow-auto rounded bg-black/40 p-2 text-xs text-rose-300">
            {error.detail}
          </pre>
        ) : null}
      </div>
    );
  }
  if (!result) {
    return <p className="text-sm text-zinc-500">Ready to pack. Click Validate.</p>;
  }
  const { params } = result;
  return (
    <div data-testid="upload-validate-result" className="flex flex-col gap-2 text-sm text-zinc-300">
      <div className="flex justify-between">
        <span>Content hash</span>
        <code className="text-xs text-zinc-500">{params.content_hash.slice(0, 12)}…</code>
      </div>
      <div className="flex justify-between">
        <span>Compressed</span>
        <span>{(params.compressed_size / 1024).toFixed(1)} KB</span>
      </div>
      <div className="flex justify-between">
        <span>Uncompressed</span>
        <span>{(params.uncompressed_size / 1024).toFixed(1)} KB</span>
      </div>
    </div>
  );
}

// ── Step 4 — Publish (progress + result) ────────────────────────────────

export interface PublishStepProps {
  progress: UploadPublishProgress | null;
  result: UploadPublishResult | null;
  error: ShareWsError | null;
}

export function PublishStep({ progress, result, error }: PublishStepProps) {
  if (error) {
    return (
      <div
        data-testid="upload-publish-error"
        className="rounded-md border border-rose-800 bg-rose-950/30 p-3 text-sm text-rose-200"
      >
        {error.message}
      </div>
    );
  }
  if (result) {
    return (
      <div
        data-testid="upload-publish-success"
        className="flex flex-col gap-2 rounded-md border border-emerald-800 bg-emerald-950/30 p-3 text-sm text-emerald-200"
      >
        <span className="font-medium">
          {result.params.status === 'created' ? 'Published' : 'Already up to date'}
        </span>
        <span className="text-xs text-emerald-300">
          Artifact ID: <code>{result.params.artifact_id}</code>
        </span>
      </div>
    );
  }
  if (progress) {
    // upload.publishProgress emits the 3 phases pack | sanitize | upload.
    // Map to <InstallProgress />'s wire phases by reusing the same primitive —
    // its phase enum covers download|verify|sanitize|write|done|error, which
    // isn't a 1:1 match, but the visual is the same (labeled progress bar).
    return (
      <InstallProgress
        phase="sanitize"
        done={progress.params.done}
        total={progress.params.total}
        label={`Publishing — ${progress.params.phase}`}
      />
    );
  }
  return <p className="text-sm text-zinc-500">Ready to publish.</p>;
}
