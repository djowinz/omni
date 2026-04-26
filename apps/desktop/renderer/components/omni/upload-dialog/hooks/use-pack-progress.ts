/**
 * usePackProgress — accumulates `upload.packProgress` frames into a per-stage
 * status record for the Step 3 Packing UI (INV-7.3.1–7.3.8).
 *
 * Subscribes to `upload.packProgress` via {@link useShareWs.subscribe}. Each
 * frame's `params: { stage, status, detail }` flips the named stage's status
 * and replaces `latestDetail`. All five stages start in the local pseudo-state
 * `'pending'`; the wire-level `StageStatus` is `'running' | 'passed' | 'failed'`
 * only.
 *
 * Returns:
 *   - `stages`: Record<PackStage, StageStatusOrPending>
 *   - `latestDetail`: the most recent frame's `detail` (null until the first
 *     frame with a non-null detail arrives)
 *
 * The hook does NOT manage violations — those arrive on the terminal error
 * envelope of `upload.publish`/`upload.update` and are passed into
 * `<Packing />` separately via props.
 */

import { useEffect, useState } from 'react';
import { useShareWs } from '../../../../hooks/use-share-ws';
import type { PackStage } from '@omni/shared-types';
import type { StageStatus } from '@omni/shared-types';

/** Local pseudo-state added on top of the wire-level {@link StageStatus}. */
export type PackStageStatus = StageStatus | 'pending';

/** Ordered list of stages the host emits, matching INV-7.3.3. */
export const PACK_STAGES: readonly PackStage[] = [
  'schema',
  'content-safety',
  'asset',
  'dependency',
  'size',
] as const;

export interface UsePackProgressResult {
  stages: Record<PackStage, PackStageStatus>;
  latestDetail: string | null;
}

function initialStages(): Record<PackStage, PackStageStatus> {
  return {
    schema: 'pending',
    'content-safety': 'pending',
    asset: 'pending',
    dependency: 'pending',
    size: 'pending',
  };
}

export function usePackProgress(): UsePackProgressResult {
  const ws = useShareWs();
  const [stages, setStages] = useState<Record<PackStage, PackStageStatus>>(initialStages);
  const [latestDetail, setLatestDetail] = useState<string | null>(null);

  useEffect(() => {
    const unsubscribe = ws.subscribe('upload.packProgress', (frame) => {
      const { stage, status, detail } = frame.params;
      setStages((prev) => ({ ...prev, [stage]: status }));
      if (detail !== null) setLatestDetail(detail);
    });
    return unsubscribe;
  }, [ws]);

  return { stages, latestDetail };
}
