/**
 * useUploadMachine — finite state machine for the new UploadDialog.
 *
 * Source of truth for "which step is the dialog on, and what should the
 * footer / header / stepper render right now". Owned by `index.tsx`; step
 * components (`steps/source-picker.tsx`, `steps/review.tsx`, …) receive
 * `state` + `actions` as props.
 *
 * Steps (INV-7.0.5): `select` → `details` → `packing` → `upload`.
 *
 * Modes (INV-7.0.4 + §7.5):
 *   - `create` — default for fresh publishes.
 *   - `update` — auto-detected when the selected entry has a sidecar AND
 *     `sidecar.author_pubkey_hex === currentPubkey` (the running identity).
 *     Mode flips header copy, version-bump field, and worker request path.
 *
 * In-flight guard (`next()`): a single boolean prevents double-fire when the
 * primary CTA is double-clicked. Async transitions (the future packing /
 * upload steps) hold the guard until they resolve. This task's `next()` is
 * synchronous (just advances the step counter) but the guard is in place so
 * the contract is stable when A1.4/A1.5 wire real async work.
 *
 * Reset on close: `index.tsx` calls `actions.reset()` from a `useEffect`
 * keyed on the dialog's `open` prop. The reducer's RESET action clears all
 * fields back to `INITIAL_STATE`.
 */

import { useCallback, useReducer, useRef } from 'react';
import type { PublishablesEntry } from '@omni/shared-types';

// ── Public types ─────────────────────────────────────────────────────────────

export type Step = 'select' | 'details' | 'packing' | 'upload';
export type Mode = 'create' | 'update';
export type UploadState = 'idle' | 'in-flight' | 'success' | 'error';

export interface UploadMachineState {
  /** Current step (drives content slot + stepper highlight + footer label). */
  step: Step;
  /** Create vs update; flips header copy + worker request path (§7.5). */
  mode: Mode;
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
}

export interface UploadMachineActions {
  /** Select (or clear) a workspace entry; triggers update-mode auto-detection. */
  selectItem: (entry: PublishablesEntry | null) => void;
  /** Set the running identity's pubkey hex (used for mode detection). */
  setCurrentPubkey: (pubkey: string | null) => void;
  /** Advance to the next step; in-flight guarded so double-clicks no-op. */
  next: () => Promise<void>;
  /** Go back one step. No-op on `select`. */
  back: () => void;
  /** Reset the entire machine to its initial state (called on dialog close). */
  reset: () => void;
}

// ── Step ordering ────────────────────────────────────────────────────────────

const STEP_ORDER: Step[] = ['select', 'details', 'packing', 'upload'];
const STEP_INDEX: Record<Step, number> = {
  select: 0,
  details: 1,
  packing: 2,
  upload: 3,
};

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

// ── Reducer ──────────────────────────────────────────────────────────────────

type Action =
  | { type: 'SELECT_ITEM'; entry: PublishablesEntry | null }
  | { type: 'SET_CURRENT_PUBKEY'; pubkey: string | null }
  | { type: 'NEXT' }
  | { type: 'BACK' }
  | { type: 'RESET' };

const INITIAL_STATE: UploadMachineState = {
  step: 'select',
  mode: 'create',
  selected: null,
  completedSteps: [],
  stepError: null,
  uploadState: 'idle',
  primaryDisabled: true, // Step 1 starts disabled until an item is picked.
  currentPubkey: null,
};

function reducer(state: UploadMachineState, action: Action): UploadMachineState {
  switch (action.type) {
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
      // Reset stepError when leaving a step; the next step starts clean.
      // primaryDisabled defaults true on entry until the step's own
      // gating logic enables it (selection, validation, etc.).
      return {
        ...state,
        step: nextStep,
        completedSteps,
        stepError: null,
        primaryDisabled: true,
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
      return {
        ...state,
        step: prevStep,
        completedSteps,
        stepError: null,
        primaryDisabled: false,
      };
    }
    case 'RESET':
      return INITIAL_STATE;
  }
}

// ── Hook ─────────────────────────────────────────────────────────────────────

export function useUploadMachine(): {
  state: UploadMachineState;
  actions: UploadMachineActions;
} {
  const [state, dispatch] = useReducer(reducer, INITIAL_STATE);

  // In-flight guard: a ref (not state) so re-renders don't lose the
  // pending flag and the guard survives the brief window between the
  // dispatch and the state update.
  const inFlightRef = useRef(false);

  const selectItem = useCallback((entry: PublishablesEntry | null) => {
    dispatch({ type: 'SELECT_ITEM', entry });
  }, []);

  const setCurrentPubkey = useCallback((pubkey: string | null) => {
    dispatch({ type: 'SET_CURRENT_PUBKEY', pubkey });
  }, []);

  const next = useCallback(async () => {
    if (inFlightRef.current) {
      // Double-click swallowed; the previous call hasn't released the guard.
      return;
    }
    inFlightRef.current = true;
    try {
      // Yield to the microtask queue once before dispatching. Today's NEXT
      // is synchronous, but the yield ensures concurrent calls within the
      // same task tick (typical double-click behaviour) hit the guard
      // instead of slipping through. Future async transitions (packing
      // start, upload publish) will await real work here; the guard is
      // held across the full async window either way.
      await Promise.resolve();
      dispatch({ type: 'NEXT' });
    } finally {
      inFlightRef.current = false;
    }
  }, []);

  const back = useCallback(() => {
    dispatch({ type: 'BACK' });
  }, []);

  const reset = useCallback(() => {
    inFlightRef.current = false;
    dispatch({ type: 'RESET' });
  }, []);

  return {
    state,
    actions: { selectItem, setCurrentPubkey, next, back, reset },
  };
}
