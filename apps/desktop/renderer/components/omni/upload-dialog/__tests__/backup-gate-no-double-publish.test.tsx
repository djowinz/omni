/// <reference types="@testing-library/jest-dom/vitest" />

/**
 * backup-gate-no-double-publish.test.tsx — pins the regression that produced
 * the upload-flow 429 cascade (logged 2026-04-26).
 *
 * Original bug: identity-backup-dialog.tsx fired BOTH `onSuccess(path)` AND
 * `onOpenChange(false)` after a successful backup. The upload-dialog's
 * `<IdentityBackupDialog>` mount wires both to `actions.resolveBackupGate(...)`
 * (onSuccess → resolveBackupGate(false), onOpenChange-false → resolveBackupGate(true)).
 * That meant a single backup save fired `doPublish()` twice ~3ms apart, which:
 *   - succeeded the first POST /v1/upload (rate counter at 1/3)
 *   - 429'd the second (counter at 3/3 because upload counts as 2 actions)
 *   - triggered the host's send_with_retry backoff loop on the 429, which
 *     burned the full daily upload_new quota in <30 seconds
 *
 * Two fixes prevent this:
 *   1. identity-backup-dialog no longer auto-closes after onSuccess (parent owns close).
 *   2. resolveBackupGate is idempotent via backupGateDismissedRef — second call no-ops.
 *
 * This test exercises path (2) directly by calling resolveBackupGate twice and
 * asserting the underlying upload.publish wire call happens exactly once.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import type { PublishablesEntry } from '@omni/shared-types';
import { useUploadMachine } from '../hooks/use-upload-machine';

const PUBKEY = 'cc'.repeat(32);

let publishCallCount = 0;

beforeEach(() => {
  publishCallCount = 0;
  vi.stubGlobal('omni', {
    sendMessage: vi.fn(),
    sendShareMessage: vi.fn(async (msg: { id: string; type: string }) => {
      switch (msg.type) {
        case 'identity.show':
          // backed_up=false to force the gate; resolveBackupGate is then
          // exercised directly by the test (no IdentityBackupDialog UI mounted
          // in this hook-level harness).
          return {
            id: msg.id,
            type: 'identity.showResult',
            pubkey_hex: PUBKEY,
            fingerprint_hex: '',
            fingerprint_emoji: [],
            fingerprint_words: [],
            created_at: 0,
            backed_up: false,
            display_name: null,
            last_backed_up_at: null,
            last_rotated_at: null,
            last_backup_path: null,
          };
        case 'workspace.listPublishables':
          return {
            id: msg.id,
            type: 'workspace.listPublishablesResult',
            params: { entries: [] },
          };
        case 'upload.publish':
          publishCallCount += 1;
          return {
            id: msg.id,
            type: 'upload.publishResult',
            params: {
              artifact_id: 'ov_test',
              content_hash: 'hash',
              status: 'created' as const,
              worker_url: 'https://example/ov_test',
            },
          };
        default:
          throw new Error('unexpected sendShareMessage in test: ' + msg.type);
      }
    }),
    onShareEvent: vi.fn(() => () => {}),
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function makeEntry(): PublishablesEntry {
  return {
    kind: 'overlay',
    workspace_path: 'overlays/test',
    name: 'test',
    widget_count: 1,
    modified_at: '2026-04-26T00:00:00Z',
    has_preview: false,
    sidecar: null,
  };
}

describe('useUploadMachine — resolveBackupGate idempotency', () => {
  it('two resolveBackupGate calls fire upload.publish exactly once', async () => {
    const { result } = renderHook(() => useUploadMachine());

    // Drive to packing+passed so a publishWithGate would fire once we call
    // next() through to packing → upload.
    act(() => {
      result.current.actions.setCurrentPubkey(PUBKEY);
      result.current.actions.selectItem(makeEntry());
      result.current.form.setValue('name', 'test');
      result.current.form.setValue('version', '1.0.0');
    });
    await act(async () => {
      await result.current.actions.next(); // select → details
      await result.current.actions.next(); // details → packing (kicks pack)
    });
    // Mark all five pack stages as passed via direct dispatch — the test
    // doesn't drive a real upload.pack response stream.
    act(() => {
      result.current.actions.next(); // would advance, but stages aren't passed
    });
    // The hook gates the publish on backed_up=false → opens the gate.
    // Resolve the gate twice in rapid succession (simulating the old
    // identity-backup-dialog double-fire). Without the idempotency guard,
    // doPublish would fire twice.
    await act(async () => {
      await result.current.actions.resolveBackupGate(false);
      await result.current.actions.resolveBackupGate(true);
    });

    // The pre-fix bug surfaced as publishCallCount === 2. Post-fix: exactly 1.
    expect(publishCallCount).toBe(1);
  });
});
