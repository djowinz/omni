/**
 * useUploadMachine — finite state machine for the new UploadDialog.
 *
 * Source of truth for "which step is the dialog on, which mode, what's the
 * publish/pack state, and which CTA should the footer render". Owned by
 * `index.tsx`; step components (`steps/source-picker.tsx`, `steps/review.tsx`,
 * `steps/packing.tsx`, `steps/publish.tsx`) receive narrowed `state` + `actions`
 * subsets as props.
 *
 * Steps (INV-7.0.5): `select` → `details` → `packing` → `upload`.
 *
 * Modes (INV-7.0.4 + §7.5):
 *   - `create` — default for fresh publishes.
 *   - `update` — auto-detected when the selected entry has a sidecar AND
 *     `sidecar.author_pubkey_hex === currentPubkey` (the running identity).
 *     Mode flips header copy, version-bump field, and worker request path.
 *
 * Per-step transitions (T-A2.1, OWI-41 — wired to real Share-WS calls):
 *   - select → details: synchronous advance.
 *   - details → packing: validate the form via `form.trigger()`, advance,
 *     then call `upload.pack` to drive the packProgress stream that
 *     `usePackProgress` subscribes to. Failure surfaces in the Packing
 *     summary card; the footer's Publish CTA stays disabled until all
 *     five stages pass (INV-7.3.8).
 *   - packing → upload: fires `upload.publish` (create) or `upload.update`
 *     (update). Subscribes to `upload.publishProgress` for the Step 4
 *     uploading view. Resolves into success/error UI.
 *   - upload (success) → close: footer's Done button calls `onOpenChange(false)`
 *     directly; this machine doesn't model that transition.
 *   - upload (error) → packing on Back; Retry re-fires the publish call.
 *
 * Identity backup gate: before the first publish/update of a session, the
 * machine consults `identity.show.backed_up`. When `false` AND the user
 * hasn't dismissed the gate this session, the publish pauses and surfaces
 * `state.backupGateOpen = true` so `index.tsx` can mount the
 * IdentityBackupDialog. Resolving the gate (`resolveBackupGate`) clears
 * the flag, marks the session as having dismissed the gate, and resumes
 * the publish. `identity.show` errors are swallowed — host will surface
 * a structured error if signing actually fails.
 *
 * In-flight guard (`next()`): a single ref-backed boolean prevents
 * double-fire when the primary CTA is double-clicked (T-A2.2 / OWI-42's
 * test pins this contract). Async transitions (pack call, publish call)
 * hold the guard until they resolve.
 *
 * Reset on close: `index.tsx` calls `actions.reset()` from a `useEffect`
 * keyed on the dialog's `open` prop. The reducer's RESET action clears all
 * fields back to `INITIAL_STATE` and releases the in-flight guard.
 */

import { useCallback, useEffect, useMemo, useReducer, useRef } from 'react';
import { useForm, type UseFormReturn } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import type { PackStage, PublishablesEntry } from '@omni/shared-types';
import { useShareWs } from '../../../../hooks/use-share-ws';
import { getCustomPreview } from '../../../../lib/indexed-db/custom-preview-store';
import {
  DEFAULT_FORM,
  UploadFormSchema,
  type UploadFormValues,
} from '../../../../lib/upload-form-schema';
import type {
  ShareWsError,
  UploadPackResult,
  UploadPublishResult,
} from '../../../../lib/share-types';
import type { PackingViolation } from '../steps/packing-violations-card';
import type { PublishError, PublishProgress, PublishResult } from '../steps/publish';

// ── Public types ─────────────────────────────────────────────────────────────

export type Step = 'select' | 'details' | 'packing' | 'upload';
export type Mode = 'create' | 'update';
export type UploadState = 'idle' | 'in-flight' | 'success' | 'error';
export type SelectedKind = 'overlay' | 'theme';

export type PackStageStatus = 'pending' | 'running' | 'passed' | 'failed';

export interface UploadMachineState {
  /** Current step (drives content slot + stepper highlight + footer label). */
  step: Step;
  /** Create vs update; flips header copy + worker request path (§7.5). */
  mode: Mode;
  /** Type-card selection on Step 1 (drives the filtered list rows). */
  selectedKind: SelectedKind | null;
  /** Currently selected workspace publishable. */
  selected: PublishablesEntry | null;
  /** 0-indexed list of steps the user has cleared (for the Stepper ✓ glyph). */
  completedSteps: number[];
  /** When non-null, stepper renders the current pill in error/warning style. */
  stepError: 'error' | 'warning' | null;
  /** Drives the footer's primary CTA label (Continue/Publish/Done/Retry). */
  uploadState: UploadState;
  /** Whether the primary CTA should be disabled. */
  primaryDisabled: boolean;
  /** Cached identity pubkey hex; used for update-mode auto-detection. */
  currentPubkey: string | null;
  /** Per-stage Packing pipeline status. Fed by `upload.packProgress` frames. */
  packStages: Record<PackStage, PackStageStatus>;
  /**
   * Aggregate Dependency Check violations. Empty unless the host's terminal
   * `upload.publish` / `upload.update` error envelope surfaces them.
   */
  packViolations: PackingViolation[];
  /** Step 4 publish progress. */
  publishProgress: PublishProgress | null;
  /** Step 4 publish success result. */
  publishResult: PublishResult | null;
  /** Step 4 publish error envelope. */
  publishError: PublishError | null;
  /**
   * True when the IdentityBackupDialog should be shown — set right before
   * a publish call when `identity.show.backed_up === false` and the gate
   * hasn't been dismissed this session.
   */
  backupGateOpen: boolean;
  /**
   * Set by `ReviewPreviewImage` when the user-uploaded preview lands in
   * the moderation-rejected state. While true, the footer's primary CTA
   * stays disabled even on Step 2 — without this gate the user could
   * advance past a rose-chrome "Image can't be used" tile and publish
   * (the host would still derive the actual thumbnail server-side, so
   * nothing breaks technically, but the UX is misleading). Cleared on
   * any non-moderation state and on RESET / SELECT_ITEM.
   */
  previewBlocked: boolean;
}

export interface UploadMachineActions {
  /** Step 1 — type-card click. */
  selectKind: (kind: SelectedKind) => void;
  /** Step 1 — list row click. */
  selectItem: (entry: PublishablesEntry | null) => void;
  /** Set the running identity's pubkey hex (used for mode detection). */
  setCurrentPubkey: (pubkey: string | null) => void;
  /** Advance to the next step; in-flight guarded so double-clicks no-op. */
  next: () => Promise<void>;
  /** Go back one step. No-op on `select`. */
  back: () => void;
  /** Reset the entire machine to its initial state (called on dialog close). */
  reset: () => void;
  /** Step 3 — re-run the full Packing pipeline (Retry button). */
  retryPack: () => Promise<void>;
  /** Step 4 — re-fire the upload.publish/upload.update call after a failure. */
  retryPublish: () => Promise<void>;
  /** Step 4 recovery — fold AuthorNameConflict into an upload.update with bump=patch. */
  linkAndUpdate: (existingArtifactId: string) => Promise<void>;
  /** Step 4 recovery — bounce back to Step 2 with the Name field for a rename. */
  renameAndPublishNew: () => void;
  /**
   * Resolve the first-publish backup gate. `dismissed=true` marks the
   * session as having seen the gate and resumes the paused publish.
   */
  resolveBackupGate: (dismissed: boolean) => Promise<void>;
  /**
   * Set by Step 2's `ReviewPreviewImage` to (un)gate the footer's primary
   * CTA on the moderation-rejected preview state. See
   * `UploadMachineState.previewBlocked` for rationale.
   */
  setPreviewBlocked: (blocked: boolean) => void;
}

// ── Step ordering ────────────────────────────────────────────────────────────

const STEP_ORDER: Step[] = ['select', 'details', 'packing', 'upload'];
const STEP_INDEX: Record<Step, number> = {
  select: 0,
  details: 1,
  packing: 2,
  upload: 3,
};

// All five host-emitted pack stages. Mirrors `PACK_STAGES` in
// `use-pack-progress.ts`; duplicated here so the reducer doesn't import a
// hook module from a non-hook context.
const PACK_STAGE_KEYS: readonly PackStage[] = [
  'schema',
  'content-safety',
  'asset',
  'dependency',
  'size',
] as const;

function initialPackStages(): Record<PackStage, PackStageStatus> {
  return {
    schema: 'pending',
    'content-safety': 'pending',
    asset: 'pending',
    dependency: 'pending',
    size: 'pending',
  };
}

function allPackStagesPassed(stages: Record<PackStage, PackStageStatus>): boolean {
  return PACK_STAGE_KEYS.every((s) => stages[s] === 'passed');
}

// ── Mode detection ───────────────────────────────────────────────────────────

/**
 * Returns 'update' when the entry has a sidecar AND its
 * `author_pubkey_hex` equals the running identity's pubkey. Anything else
 * (no sidecar, no current pubkey, mismatched author) → 'create'.
 *
 * Matches INV-7.5.* trigger condition + INV-7.6.4's "different identity →
 * new artifact" rule. Comparison is byte-equality on the lowercase hex
 * string; both sides come from `pubkey_hex` fields the host already
 * normalizes, so no per-call lower-casing is required here.
 */
export function detectMode(entry: PublishablesEntry | null, currentPubkey: string | null): Mode {
  if (!entry || !entry.sidecar || !currentPubkey) {
    return 'create';
  }
  return entry.sidecar.author_pubkey_hex === currentPubkey ? 'update' : 'create';
}

// ── Custom preview IDB read ─────────────────────────────────────────────────

/**
 * Load the user-uploaded Step 2 Preview Image from IndexedDB and base64-
 * encode it for the WS surface. Returns `null` when no custom preview was
 * persisted (the auto-generated thumbnail will be used). Errors from the
 * IDB read are also coalesced to `null` so a flaky persistence layer
 * never blocks publish — worst case the user gets the auto-render they
 * would have gotten without ever picking a custom image.
 *
 * The host-side `share::moderation::check_image` re-runs the gate on these
 * bytes (renderer-side moderation is advisory per INV-7.7.2) and either
 * accepts them as the artifact thumbnail or fails the publish with
 * `Moderation:ServerRejected`. Both renderer & host enforce the 2 MB cap
 * so an oversized image gets rejected at whichever layer sees it first.
 */
async function loadCustomPreviewB64(workspacePath: string): Promise<string | null> {
  let record: Awaited<ReturnType<typeof getCustomPreview>> = null;
  try {
    record = await getCustomPreview(workspacePath);
  } catch {
    return null;
  }
  if (!record) return null;
  const buf = await record.blob.arrayBuffer();
  const bytes = new Uint8Array(buf);
  // Encode in 32 KB chunks so very-large bytes don't blow the call stack
  // through `String.fromCharCode.apply` — same pattern as
  // `review-preview-image.tsx::blobToBase64`.
  const chunkSize = 32 * 1024;
  let binary = '';
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const chunk = bytes.subarray(i, i + chunkSize);
    binary += String.fromCharCode.apply(null, Array.from(chunk));
  }
  return btoa(binary);
}

// ── Reducer ──────────────────────────────────────────────────────────────────

type Action =
  | { type: 'SELECT_KIND'; kind: SelectedKind }
  | { type: 'SELECT_ITEM'; entry: PublishablesEntry | null }
  | { type: 'SET_CURRENT_PUBKEY'; pubkey: string | null }
  | { type: 'NEXT' }
  | { type: 'BACK' }
  | { type: 'RESET' }
  | { type: 'PACK_PROGRESS'; stage: PackStage; status: PackStageStatus }
  | { type: 'PACK_RESET' }
  | { type: 'PACK_FAILURE'; violations: PackingViolation[] }
  | { type: 'PUBLISH_START' }
  | { type: 'PUBLISH_PROGRESS'; progress: PublishProgress }
  | { type: 'PUBLISH_SUCCESS'; result: PublishResult }
  | { type: 'PUBLISH_ERROR'; error: PublishError }
  | { type: 'JUMP_TO_DETAILS' }
  | { type: 'BACKUP_GATE'; open: boolean }
  | { type: 'SET_PREVIEW_BLOCKED'; blocked: boolean };

const INITIAL_STATE: UploadMachineState = {
  step: 'select',
  mode: 'create',
  selectedKind: null,
  selected: null,
  completedSteps: [],
  stepError: null,
  uploadState: 'idle',
  primaryDisabled: true, // Step 1 starts disabled until an item is picked.
  currentPubkey: null,
  packStages: initialPackStages(),
  packViolations: [],
  publishProgress: null,
  publishResult: null,
  publishError: null,
  backupGateOpen: false,
  previewBlocked: false,
};

function reducer(state: UploadMachineState, action: Action): UploadMachineState {
  switch (action.type) {
    case 'SELECT_KIND': {
      // Switching kinds clears the selected item — the list rows below the
      // type-card grid filter on `selectedKind`, so a stale selection from
      // the other kind would still highlight as "selected" once filtered out.
      return {
        ...state,
        selectedKind: action.kind,
        selected: state.selectedKind === action.kind ? state.selected : null,
        primaryDisabled:
          state.step === 'select'
            ? state.selectedKind === action.kind
              ? state.selected === null
              : true
            : state.primaryDisabled,
      };
    }
    case 'SELECT_ITEM': {
      const mode = detectMode(action.entry, state.currentPubkey);
      // Primary CTA enables once the user has selected an item on Step 1.
      const primaryDisabled =
        state.step === 'select' ? action.entry === null : state.primaryDisabled;
      return {
        ...state,
        selected: action.entry,
        mode,
        primaryDisabled,
      };
    }
    case 'SET_CURRENT_PUBKEY': {
      // Re-derive mode against the freshly known pubkey — order of pubkey
      // arrival vs item selection isn't guaranteed (identity.show races
      // workspace.listPublishables on first dialog open).
      const mode = detectMode(state.selected, action.pubkey);
      return {
        ...state,
        currentPubkey: action.pubkey,
        mode,
      };
    }
    case 'NEXT': {
      const idx = STEP_INDEX[state.step];
      if (idx >= STEP_ORDER.length - 1) {
        return state;
      }
      const nextStep = STEP_ORDER[idx + 1];
      const completedSteps = state.completedSteps.includes(idx)
        ? state.completedSteps
        : [...state.completedSteps, idx];
      // Per-step entry primaryDisabled gating:
      //   - details: form is pre-filled (DEFAULT_FORM has empty Name but
      //     spec INV-7.2.8 footer is "Continue ›" without disabled-on-empty
      //     gating — validation runs at next() time via form.trigger()).
      //   - packing: stays disabled until allPackStagesPassed (INV-7.3.8);
      //     `next()` issues PACK_PROGRESS dispatches that flip it to false.
      //   - upload: footer state derives from uploadState; primaryDisabled
      //     is meaningless during 'in-flight' (Cancel-only) and re-enabled
      //     by PUBLISH_SUCCESS / PUBLISH_ERROR.
      let primaryDisabled = false;
      if (nextStep === 'packing') primaryDisabled = true;
      if (nextStep === 'upload') primaryDisabled = true;
      return {
        ...state,
        step: nextStep,
        completedSteps,
        stepError: null,
        primaryDisabled,
      };
    }
    case 'BACK': {
      const idx = STEP_INDEX[state.step];
      if (idx <= 0) {
        return state;
      }
      const prevStep = STEP_ORDER[idx - 1];
      // Going back un-completes the step we're returning to (it's now
      // active again). Drop it from completedSteps so the stepper renders
      // the active style, not the completed ✓.
      const completedSteps = state.completedSteps.filter((i) => i !== idx - 1);
      // Returning to Step 1 from Step 2 re-uses the previous selection, so
      // the primary CTA stays enabled. Returning to Step 3 from Step 4
      // reverts to packing-stage gating (allPassed → enable).
      let primaryDisabled = false;
      if (prevStep === 'packing') {
        primaryDisabled = !allPackStagesPassed(state.packStages);
      }
      // Coming back from Step 4 (error retry path) clears the publish error
      // so the next attempt starts clean.
      const clearedPublish =
        state.step === 'upload'
          ? {
              uploadState: 'idle' as UploadState,
              publishProgress: null,
              publishResult: null,
              publishError: null,
            }
          : {};
      return {
        ...state,
        ...clearedPublish,
        step: prevStep,
        completedSteps,
        stepError: null,
        primaryDisabled,
      };
    }
    case 'RESET':
      return {
        ...INITIAL_STATE,
        // Re-construct the initial pack stages object so dispatches don't
        // accidentally share a frozen literal across multiple resets.
        packStages: initialPackStages(),
      };
    case 'PACK_PROGRESS': {
      const packStages = { ...state.packStages, [action.stage]: action.status };
      const allPassed = allPackStagesPassed(packStages);
      // Only flip the footer's primary disabled while we're parked on the
      // packing step — past a transition out (e.g. user already clicked
      // Publish and we're in 'upload'), late frames must not re-enable a
      // CTA that's no longer relevant.
      const primaryDisabled = state.step === 'packing' ? !allPassed : state.primaryDisabled;
      // First failure marks the stepper pill red.
      const stepError =
        action.status === 'failed' && state.step === 'packing' ? 'error' : state.stepError;
      return {
        ...state,
        packStages,
        primaryDisabled,
        stepError,
      };
    }
    case 'PACK_RESET':
      return {
        ...state,
        packStages: initialPackStages(),
        packViolations: [],
        stepError: state.step === 'packing' ? null : state.stepError,
      };
    case 'PACK_FAILURE': {
      // Surface the dependency violations that arrived on the publish error
      // envelope. Mark the dependency stage as failed so the Packing UI
      // renders the aggregate violations card (INV-7.3.7 / 7.8.5).
      const packStages = { ...state.packStages, dependency: 'failed' as PackStageStatus };
      return {
        ...state,
        packStages,
        packViolations: action.violations,
        primaryDisabled: state.step === 'packing' ? true : state.primaryDisabled,
        stepError: state.step === 'packing' ? 'error' : state.stepError,
      };
    }
    case 'PUBLISH_START':
      return {
        ...state,
        uploadState: 'in-flight',
        publishProgress: null,
        publishResult: null,
        publishError: null,
        // Step 4 footer hides the primary while in-flight (INV-7.4.5);
        // disabling here is belt + suspenders.
        primaryDisabled: true,
      };
    case 'PUBLISH_PROGRESS':
      return {
        ...state,
        publishProgress: action.progress,
      };
    case 'PUBLISH_SUCCESS':
      return {
        ...state,
        uploadState: 'success',
        publishResult: action.result,
        publishError: null,
        primaryDisabled: false,
      };
    case 'PUBLISH_ERROR':
      return {
        ...state,
        uploadState: 'error',
        publishResult: null,
        publishError: action.error,
        // Retry button is the primary on error (INV-7.4.5) — keep enabled.
        primaryDisabled: false,
        // Step indicator's Upload pill switches to Error variant (INV-7.4.4).
        stepError: state.step === 'upload' ? 'error' : state.stepError,
      };
    case 'JUMP_TO_DETAILS': {
      // Used by `renameAndPublishNew` — bounce back to Step 2 so the user
      // can edit the Name field. Clears Step 4's publish error and the
      // intermediate completed-step markers past Step 2.
      return {
        ...state,
        step: 'details',
        completedSteps: state.completedSteps.filter((i) => i < 1),
        stepError: null,
        uploadState: 'idle',
        publishProgress: null,
        publishResult: null,
        publishError: null,
        primaryDisabled: false,
      };
    }
    case 'BACKUP_GATE':
      return { ...state, backupGateOpen: action.open };
    case 'SET_PREVIEW_BLOCKED':
      // Cheap idempotent guard so a re-fire from an effect with a stable
      // dependency doesn't churn the reducer (and re-render every consumer).
      if (state.previewBlocked === action.blocked) return state;
      return { ...state, previewBlocked: action.blocked };
  }
}

// ── Helper: derive a PublishError from a ShareWsError ────────────────────────

function toPublishError(err: unknown): PublishError {
  const e = err as ShareWsError | undefined;
  return {
    code: e?.code ?? 'UNKNOWN',
    message: e?.message ?? 'Upload failed.',
    detail: e?.detail ?? '',
  };
}

// ── Helper: extract dependency violations from a packErr ─────────────────────

interface MaybeViolationsBag {
  detail?: unknown;
  violations?: unknown;
}

/**
 * Best-effort extraction of dependency violations from a `upload.pack` /
 * `upload.publish` error envelope. The host emits violations via the
 * structured-error `detail` field as JSON; older code paths may carry a
 * `violations` array directly. Returns `[]` when neither shape is present.
 */
function extractPackViolations(err: unknown): PackingViolation[] {
  if (!err || typeof err !== 'object') return [];
  const bag = err as MaybeViolationsBag;
  // Direct array on the envelope (host's preferred shape, post-OWI-44).
  if (Array.isArray(bag.violations)) {
    return bag.violations.filter(isViolation);
  }
  // Detail blob — try parse-as-JSON, fall back to empty.
  if (typeof bag.detail === 'string') {
    try {
      const parsed = JSON.parse(bag.detail) as { violations?: unknown };
      if (Array.isArray(parsed.violations)) {
        return parsed.violations.filter(isViolation);
      }
    } catch {
      /* fall through */
    }
  }
  return [];
}

function isViolation(v: unknown): v is PackingViolation {
  if (!v || typeof v !== 'object') return false;
  const r = v as { kind?: unknown; path?: unknown };
  return typeof r.kind === 'string' && typeof r.path === 'string';
}

// ── Public hook ──────────────────────────────────────────────────────────────

export interface UseUploadMachineOptions {
  /**
   * Optional pre-selected workspace path. When set on dialog open, the
   * machine looks up the matching `PublishablesEntry` after the workspace
   * list resolves and jumps directly to Step 2. Used by the widget-panel
   * "Publish" button so the user doesn't re-pick the active overlay.
   */
  prefilledPath?: string | null;
  /** Fired on successful publish so consumers can refresh their own state. */
  onPublished?: (params: UploadPublishResult['params']) => void;
  /**
   * Whether the parent dialog is currently open. Threaded through so the
   * prefilled-path effect can re-fire on reopen — without `open` in the
   * dep array, closing+reopening with the same `prefilledPath` value
   * (the editor's "Upload" button does this on every click for the
   * active overlay) leaves the dialog stuck on Step 1 because React's
   * effect dep comparison sees no change. INV-7.6.1 / regression
   * `prefill-on-reopen.test.tsx`.
   */
  open?: boolean;
}

export interface UseUploadMachineResult {
  state: UploadMachineState;
  actions: UploadMachineActions;
  /** React-hook-form instance for the Step 2 Review form. */
  form: UseFormReturn<UploadFormValues>;
}

export function useUploadMachine(options: UseUploadMachineOptions = {}): UseUploadMachineResult {
  const { prefilledPath = null, onPublished, open = true } = options;
  const ws = useShareWs();

  const [state, dispatch] = useReducer(reducer, INITIAL_STATE);

  const form = useForm<UploadFormValues>({
    resolver: zodResolver(UploadFormSchema),
    defaultValues: DEFAULT_FORM,
  });

  // In-flight guard: a ref (not state) so re-renders don't lose the
  // pending flag and the guard survives the brief window between the
  // dispatch and the state update.
  const inFlightRef = useRef(false);

  // Stable refs for state pieces that helpers need to read without
  // becoming dependencies of the corresponding `useCallback`. Without this
  // every `next()` reference would change on every render, defeating
  // memoization in the dialog header / footer / step components.
  const stateRef = useRef(state);
  stateRef.current = state;

  // Track whether the user already passed (or dismissed) the first-publish
  // backup gate this session. Session-scoped so reopening the dialog or
  // bouncing through the Step 2 ↔ Step 3 transitions doesn't re-prompt
  // after the first acknowledgement.
  const backupGateDismissedRef = useRef(false);

  const onPublishedRef = useRef(onPublished);
  onPublishedRef.current = onPublished;

  // ── Subscriptions ──────────────────────────────────────────────────────────

  // Pack progress subscription — long-lived for the dialog's lifetime so
  // late frames after a transition still update `state.packStages` (the
  // host emits stages as it processes them; the user sees them animate
  // in even after the Continue button has advanced past Step 2 and into
  // the Step 3 Packing UI).
  useEffect(() => {
    const unsub = ws.subscribe('upload.packProgress', (frame) => {
      const { stage, status } = frame.params;
      dispatch({ type: 'PACK_PROGRESS', stage, status });
    });
    return unsub;
  }, [ws]);

  // Publish progress subscription — same lifecycle reasoning.
  useEffect(() => {
    const unsub = ws.subscribe('upload.publishProgress', (frame) => {
      dispatch({ type: 'PUBLISH_PROGRESS', progress: frame.params });
    });
    return unsub;
  }, [ws]);

  // Identity bootstrap — fetch on first mount so update-mode auto-detection
  // can happen as soon as the user picks a workspace entry. Errors are
  // swallowed (fresh-install / NOT_IMPLEMENTED stub return values are
  // benign here; the host will surface a structured error at publish-time
  // if it actually can't sign).
  //
  // We respect any pubkey that's already been set (via `setCurrentPubkey`)
  // so test harnesses + future identity-rotation flows can pre-seed the
  // value without it being clobbered by an in-flight bootstrap.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const resp = await ws.send('identity.show', {});
        if (!cancelled && stateRef.current.currentPubkey === null) {
          dispatch({ type: 'SET_CURRENT_PUBKEY', pubkey: resp.pubkey_hex });
        }
      } catch {
        /* fresh install / stub — leave currentPubkey null */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [ws]);

  // Prefilled path → wait for the workspace list to resolve, then look up
  // the matching entry and skip to Step 2. The lookup itself is one-shot
  // per `prefilledPath` change; subsequent re-renders see the same prefilled
  // value and the `selected` state is already populated, so this is a no-op.
  const prefilledHandledRef = useRef<string | null>(null);
  useEffect(() => {
    // Bail when the dialog is closed — the parent's `open` prop drives
    // mount lifecycle but the hook stays mounted, so we use `open` as the
    // re-fire signal. Without this gate, the effect would still only run
    // once per `prefilledPath` change because `prefilledHandledRef`
    // short-circuits.
    if (!open) return;
    if (!prefilledPath || prefilledHandledRef.current === prefilledPath) return;
    prefilledHandledRef.current = prefilledPath;
    let cancelled = false;
    void (async () => {
      try {
        const resp = await ws.send('workspace.listPublishables', {});
        if (cancelled) return;
        const match = resp.params.entries.find((e) => e.workspace_path === prefilledPath);
        if (!match) return;
        dispatch({ type: 'SELECT_KIND', kind: match.kind === 'theme' ? 'theme' : 'overlay' });
        dispatch({ type: 'SELECT_ITEM', entry: match });
        // Synchronously advance into Step 2 so the picker isn't shown.
        dispatch({ type: 'NEXT' });
      } catch {
        /* silent — picker remains the user's fallback */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [prefilledPath, ws, open]);

  // Seed Step 2 form fields from the sidecar when update mode resolves.
  // The reducer's SELECT_ITEM action already flips `state.mode` to 'update'
  // when the entry's `sidecar.author_pubkey_hex` matches `currentPubkey`;
  // this effect closes the loop by populating the form so Step 2 doesn't
  // render with DEFAULT_FORM blanks. INV-7.5.3.
  //
  // Side-effect-only — no dispatch. `form.reset(...)` re-initialises the
  // react-hook-form values in one batch (cheaper than per-field setValue
  // and avoids intermediate validation-trigger storms).
  //
  // Subscribe to the form's dirty flag at render time. react-hook-form's
  // `formState` is a Proxy whose properties only become reactive when
  // read during render — reading `form.formState.isDirty` inside an
  // effect alone returns the stale initial value (`false`) because the
  // Proxy never gets a subscription notice. Destructuring here at the
  // hook's top level registers the subscription, so the value is
  // up-to-date when the prefill effect below reads it.
  const { isDirty } = form.formState;
  useEffect(() => {
    if (state.mode !== 'update') return;
    const entry = state.selected;
    const sidecar = entry?.sidecar;
    if (!entry || !sidecar) return;
    // Don't clobber values the user has already touched. Mode-flips driven
    // by late-arriving identity (`SET_CURRENT_PUBKEY`) or by the
    // `linkAndUpdate` recovery flow (which dispatches SELECT_ITEM with a
    // synthetic entry mid-publish) re-trigger this effect with a fresh
    // entry reference; without this guard, `form.reset(...)` would
    // overwrite the user's Step 2 input. INV-7.5.3 specifies prefill on
    // first entry into update mode, not unconditional re-prefill.
    if (isDirty) return;
    form.reset({
      name: entry.name,
      description: sidecar.description ?? '',
      tags: sidecar.tags ?? [],
      license: sidecar.license ?? '',
      customLicense: '',
      bump: 'patch',
      version: sidecar.version ?? '1.0.0',
    });
  }, [state.mode, state.selected, form, isDirty]);

  // ── Pack execution ─────────────────────────────────────────────────────────

  const runPack = useCallback(async () => {
    const cur = stateRef.current;
    if (!cur.selected) return;
    dispatch({ type: 'PACK_RESET' });
    const customPreviewB64 = await loadCustomPreviewB64(cur.selected.workspace_path);
    try {
      // The host emits packProgress frames as it goes; the result here is the
      // terminal pack manifest + sanitize report. We don't need its body —
      // the per-stage frames already drove the UI. We DO swallow errors
      // because pack failures surface via the per-stage 'failed' frame (the
      // host emits 'failed' before throwing) and via the publish-time error
      // envelope (which carries the structured violation list).
      await ws.send('upload.pack', {
        workspace_path: cur.selected.workspace_path,
        ...(customPreviewB64 !== null && { custom_preview_b64: customPreviewB64 }),
      });
    } catch (err) {
      // If a stage hasn't already been marked failed, mark dependency-stage
      // as failed and surface any violations carried on the envelope. The
      // dependency-check is the only stage that emits structured violations.
      const violations = extractPackViolations(err);
      if (violations.length > 0) {
        dispatch({ type: 'PACK_FAILURE', violations });
      }
    }
  }, [ws]);

  // ── Publish execution ──────────────────────────────────────────────────────

  /**
   * Builds the request param object common to publish + update. Collapses
   * the `customLicense` escape hatch into the single user-visible `license`
   * string per `lib/upload-form-schema.ts`.
   */
  function buildPublishParams() {
    const cur = stateRef.current;
    const values = form.getValues();
    if (!cur.selected) return null;
    const license =
      values.license === 'Custom' ? values.customLicense || undefined : values.license || undefined;
    return {
      workspace_path: cur.selected.workspace_path,
      bump: values.bump,
      name: values.name,
      description: values.description,
      tags: values.tags,
      license,
      version: values.version,
      omni_min_version: values.omni_min_version,
    };
  }

  const doPublish = useCallback(async () => {
    const cur = stateRef.current;
    const common = buildPublishParams();
    if (!common) return;
    dispatch({ type: 'PUBLISH_START' });
    const customPreviewB64 = cur.selected
      ? await loadCustomPreviewB64(cur.selected.workspace_path)
      : null;
    const commonWithPreview = {
      ...common,
      ...(customPreviewB64 !== null && { custom_preview_b64: customPreviewB64 }),
    };
    try {
      if (cur.mode === 'update' && cur.selected?.sidecar) {
        const resp = await ws.send('upload.update', {
          ...commonWithPreview,
          artifact_id: cur.selected.sidecar.artifact_id,
        });
        // Normalise `updated`/`unchanged` (UploadUpdateResult-only) into
        // `created` so the Step 4 success card renders uniformly.
        const status =
          resp.params.status === 'created' || resp.params.status === 'deduplicated'
            ? resp.params.status
            : 'created';
        const normalised: UploadPublishResult = {
          id: resp.id,
          type: 'upload.publishResult',
          params: {
            artifact_id: resp.params.artifact_id,
            content_hash: resp.params.content_hash,
            status,
            worker_url: resp.params.worker_url,
          },
        };
        dispatch({
          type: 'PUBLISH_SUCCESS',
          result: {
            artifact_id: normalised.params.artifact_id,
            name: common.name ?? cur.selected.name,
            kind: cur.selected.kind,
            tags: common.tags ?? [],
          },
        });
        onPublishedRef.current?.(normalised.params);
      } else {
        const resp = await ws.send('upload.publish', {
          ...commonWithPreview,
          visibility: 'public' as const,
        });
        dispatch({
          type: 'PUBLISH_SUCCESS',
          result: {
            artifact_id: resp.params.artifact_id,
            name: common.name ?? cur.selected?.name ?? '',
            kind: cur.selected?.kind ?? 'overlay',
            tags: common.tags ?? [],
          },
        });
        onPublishedRef.current?.(resp.params);
      }
    } catch (err) {
      const violations = extractPackViolations(err);
      if (violations.length > 0) {
        // Dependency violations surfaced at publish-time — bounce back to
        // Step 3 so the aggregate card can render. Dispatch order: clear
        // the publish error, mark the failed stage, jump back.
        dispatch({ type: 'PACK_FAILURE', violations });
        dispatch({ type: 'BACK' });
        return;
      }
      dispatch({ type: 'PUBLISH_ERROR', error: toPublishError(err) });
    }
  }, [form, ws]);

  // Wraps doPublish() with the first-publish backup gate. Open the
  // IdentityBackupDialog if `identity.show.backed_up === false` AND the
  // user hasn't already dismissed the gate this session.
  const publishWithGate = useCallback(async () => {
    if (backupGateDismissedRef.current) {
      await doPublish();
      return;
    }
    try {
      const identity = await ws.send('identity.show', {});
      if (!identity.backed_up) {
        // Pause; resolveBackupGate() resumes after the dialog resolves.
        dispatch({ type: 'BACKUP_GATE', open: true });
        return;
      }
    } catch {
      // identity.show failure (fresh install / stub) → proceed; host
      // surfaces a structured error if it actually can't sign.
    }
    await doPublish();
  }, [doPublish, ws]);

  // ── Public actions ─────────────────────────────────────────────────────────

  const selectKind = useCallback((kind: SelectedKind) => {
    dispatch({ type: 'SELECT_KIND', kind });
  }, []);

  const selectItem = useCallback((entry: PublishablesEntry | null) => {
    dispatch({ type: 'SELECT_ITEM', entry });
  }, []);

  const setCurrentPubkey = useCallback((pubkey: string | null) => {
    dispatch({ type: 'SET_CURRENT_PUBKEY', pubkey });
  }, []);

  const setPreviewBlocked = useCallback((blocked: boolean) => {
    dispatch({ type: 'SET_PREVIEW_BLOCKED', blocked });
  }, []);

  const next = useCallback(async () => {
    if (inFlightRef.current) {
      // Double-click swallowed; the previous call hasn't released the guard.
      return;
    }
    inFlightRef.current = true;
    try {
      const cur = stateRef.current;
      // Yield once before dispatching. Today's per-step transitions are a
      // mix of sync + async; the yield guarantees concurrent calls within
      // the same task tick (typical double-click behaviour) hit the guard
      // instead of slipping through.
      await Promise.resolve();
      switch (cur.step) {
        case 'select': {
          if (!cur.selected) return;
          dispatch({ type: 'NEXT' });
          break;
        }
        case 'details': {
          const ok = await form.trigger();
          if (!ok) return;
          dispatch({ type: 'NEXT' });
          // Kick off pack — packProgress frames stream into the reducer.
          await runPack();
          break;
        }
        case 'packing': {
          if (!allPackStagesPassed(stateRef.current.packStages)) return;
          dispatch({ type: 'NEXT' });
          await publishWithGate();
          break;
        }
        case 'upload':
          // Step 4 'Done' is wired in `index.tsx` directly to onOpenChange(false);
          // 'Retry' goes through retryPublish(). NEXT here is a no-op.
          break;
      }
    } finally {
      inFlightRef.current = false;
    }
  }, [form, publishWithGate, runPack]);

  const back = useCallback(() => {
    dispatch({ type: 'BACK' });
  }, []);

  const reset = useCallback(() => {
    inFlightRef.current = false;
    backupGateDismissedRef.current = false;
    prefilledHandledRef.current = null;
    form.reset(DEFAULT_FORM);
    dispatch({ type: 'RESET' });
  }, [form]);

  const retryPack = useCallback(async () => {
    await runPack();
  }, [runPack]);

  const retryPublish = useCallback(async () => {
    // Re-fire the same publish path (with backup-gate) that the Continue
    // button used. PUBLISH_START clears the prior error envelope so the
    // Step 4 spinner re-renders immediately.
    await publishWithGate();
  }, [publishWithGate]);

  const linkAndUpdate = useCallback(
    async (existingArtifactId: string) => {
      // Mirror the legacy recovery flow: the user is linking the artifact to
      // their identity and pushing a +1 patch. Sets the form's bump to
      // 'patch' and the (synthetic) selected entry's sidecar so doPublish
      // routes through upload.update.
      const cur = stateRef.current;
      if (!cur.selected) return;
      const synthetic: PublishablesEntry = {
        ...cur.selected,
        sidecar: {
          artifact_id: existingArtifactId,
          author_pubkey_hex: cur.currentPubkey ?? '',
          version: cur.publishError?.detail ? '0.0.0' : '0.0.0',
          last_published_at: new Date().toISOString(),
        },
      };
      dispatch({ type: 'SELECT_ITEM', entry: synthetic });
      form.setValue('bump', 'patch');
      // Re-fire as upload.update via doPublish (mode now resolves to 'update'
      // because synthetic.sidecar.author_pubkey_hex === currentPubkey).
      await publishWithGate();
    },
    [form, publishWithGate],
  );

  const renameAndPublishNew = useCallback(() => {
    dispatch({ type: 'JUMP_TO_DETAILS' });
    // Focus the Name field — jsdom doesn't render it but production browsers
    // do; querySelector keeps the action a no-op when the element isn't
    // mounted (e.g. mid-test).
    queueMicrotask(() => {
      const el = document.querySelector<HTMLInputElement>('[data-testid="upload-name"]');
      el?.focus();
    });
  }, []);

  const resolveBackupGate = useCallback(
    async (dismissed: boolean) => {
      // Idempotency guard. The IdentityBackupDialog used to fire BOTH
      // `onSuccess` and `onOpenChange(false)` on a successful backup, which
      // landed two `resolveBackupGate(...)` calls ~3ms apart and double-fired
      // doPublish — burning the daily upload quota and producing a 429 cascade.
      // identity-backup-dialog.tsx no longer auto-closes after onSuccess, but
      // this ref-backed guard prevents any future caller (or future Dialog
      // refactor) from re-introducing the same race. First call wins; the
      // second is swallowed.
      if (backupGateDismissedRef.current) return;
      backupGateDismissedRef.current = true;
      dispatch({ type: 'BACKUP_GATE', open: false });
      // Whether the user backed up successfully or explicitly dismissed,
      // the publish resumes either way (per legacy behaviour: dismiss treated
      // as "proceed without backup; host surfaces signing errors if any").
      void dismissed;
      await doPublish();
    },
    [doPublish],
  );

  const actions = useMemo<UploadMachineActions>(
    () => ({
      selectKind,
      selectItem,
      setCurrentPubkey,
      next,
      back,
      reset,
      retryPack,
      retryPublish,
      linkAndUpdate,
      renameAndPublishNew,
      resolveBackupGate,
      setPreviewBlocked,
    }),
    [
      selectKind,
      selectItem,
      setCurrentPubkey,
      next,
      back,
      reset,
      retryPack,
      retryPublish,
      linkAndUpdate,
      renameAndPublishNew,
      resolveBackupGate,
      setPreviewBlocked,
    ],
  );

  return { state, actions, form };
}

// `UploadPackResult` is referenced in the doc-comment for runPack but the
// concrete type isn't used in the runtime code; keep the import alive for
// type-only consumers.
export type { UploadPackResult };
