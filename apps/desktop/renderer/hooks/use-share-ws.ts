/**
 * useShareWs — typed WebSocket client for the Omni Share Hub message surface.
 *
 * ## send(type, params): Promise<result>
 * Request-response path. Generates a fresh request id (crypto.randomUUID),
 * routes through `window.omni.sendShareMessage({ id, type, ...params })`
 * (an `ipcRenderer.invoke` under the hood → main.ts `share:ws-message`
 * handler → hostManager.sendAndWaitById). The raw host frame is returned
 * (either a success frame or a D-004-J error envelope). This hook then
 * Zod-parses it: error envelope → throws `ShareWsError`; shape mismatch →
 * throws `ShareWsError { code: 'PARSE_FAILED' }`; success → returns validated
 * result.
 *
 * ## subscribe(type, handler): () => void
 * Subscribes to unsolicited streaming frames (install/publish/pack progress).
 * Uses the `window.omni.onShareEvent` IPC channel (see
 * `apps/desktop/main/preload.ts` + the SHARE_EVENT_TYPES forwarding in
 * `main.ts`). A single module-level subscription to `onShareEvent` is shared
 * across all hook consumers; incoming frames are dispatched to handlers keyed
 * by `frame.type`. Returns an unsubscribe that removes the handler (and the
 * underlying onShareEvent listener when no handlers remain).
 *
 * ## Design decision: dedicated IPC channel
 * Share request-response uses a dedicated `share:ws-message` channel rather
 * than the generic `ws-message` channel because:
 *   - Share messages correlate by `id` (concurrent requests — install, preview,
 *     cancel can all be in flight at once). The `ws-message` path serializes
 *     with a type-based response map that doesn't know about share types.
 *   - The D-004-J error envelope must reach the renderer verbatim so this
 *     hook can Zod-parse `{ code, kind, detail, message }`. The generic
 *     `ws-message` path flattens errors into plain Error objects, losing
 *     structured information.
 * Documented here so Wave-3b implementers don't add share types back into
 * `ws-message`'s responseTypes map.
 */

import { useMemo } from 'react';
import {
  type ShareRequestMap,
  type ShareSubscriptionMap,
  type ShareSubscriptionType,
  ShareResponseSchemas,
  ShareSubscriptionSchemas,
  ShareErrorFrameSchema,
  type ShareWsError,
} from '../lib/share-types';

type RequestType = keyof ShareRequestMap;

// Map from request type (e.g. 'explorer.list') to the response schema keyed by
// result frame type (e.g. 'explorer.listResult'). We resolve the schema at
// runtime by reading the `type` field from the returned response frame, which
// the host always includes. This avoids a redundant request→result-type map.
const RESPONSE_SCHEMAS = ShareResponseSchemas;

interface UseShareWs {
  send<T extends RequestType>(
    type: T,
    params: ShareRequestMap[T]['params'],
  ): Promise<ShareRequestMap[T]['result']>;
  subscribe<T extends ShareSubscriptionType>(
    type: T,
    handler: (frame: ShareSubscriptionMap[T]) => void,
  ): () => void;
}

// Module-level subscription registry + lazy onShareEvent subscription.
const handlers = new Map<string, Set<(frame: unknown) => void>>();
let unsubscribeOnShareEvent: (() => void) | null = null;

function ensureOnShareEventSubscribed() {
  if (unsubscribeOnShareEvent !== null) return;
  unsubscribeOnShareEvent = window.omni!.onShareEvent((frame: unknown) => {
    // Narrow shape: frame must have a `type: string` field.
    if (
      typeof frame !== 'object' ||
      frame === null ||
      typeof (frame as { type?: unknown }).type !== 'string'
    ) {
      console.warn('[useShareWs] ignoring share:event frame without string type', frame);
      return;
    }
    const typeStr = (frame as { type: string }).type;
    const bucket = handlers.get(typeStr);
    if (!bucket || bucket.size === 0) return;
    const schema = ShareSubscriptionSchemas[typeStr as ShareSubscriptionType];
    if (!schema) {
      console.warn('[useShareWs] no Zod schema for share:event type', typeStr);
      return;
    }
    const parse = schema.safeParse(frame);
    if (!parse.success) {
      console.warn(
        '[useShareWs] share:event failed schema validation',
        typeStr,
        parse.error.issues,
      );
      return;
    }
    // Deliver to all handlers. Defensive-copy so a handler removing itself mid-iteration
    // doesn't corrupt the set.
    const snapshot = Array.from(bucket);
    for (const h of snapshot) {
      try {
        h(parse.data);
      } catch (err) {
        console.error('[useShareWs] subscription handler threw', typeStr, err);
      }
    }
  });
}

function maybeUnsubscribeOnShareEvent() {
  if (handlers.size === 0 && unsubscribeOnShareEvent !== null) {
    unsubscribeOnShareEvent();
    unsubscribeOnShareEvent = null;
  }
}

export function useShareWs(): UseShareWs {
  return useMemo<UseShareWs>(
    () => ({
      async send(type, params) {
        const id = crypto.randomUUID();
        // Host dispatchers in `crates/host/src/share/ws_messages.rs` read
        // `msg.get("params")` and deserialize that nested object into the
        // per-handler param struct. DO NOT spread params at top level —
        // the dispatcher would see an empty params map and reject with
        // "missing field" for every required field.
        console.log('[useShareWs.send] →', { id, type, params });
        const response = await window.omni!.sendShareMessage({ id, type, params });
        console.log('[useShareWs.send] ← raw response for', type, response);

        // First check for error envelope.
        const errParse = ShareErrorFrameSchema.safeParse(response);
        if (errParse.success) {
          console.error('[useShareWs.send] error envelope for', type, errParse.data.error);
          throw errParse.data.error as ShareWsError;
        }

        // Resolve the response schema by the `type` field on the returned frame.
        // The host always includes `type` on response frames (e.g. 'explorer.listResult').
        const responseType =
          typeof response === 'object' && response !== null && typeof response.type === 'string'
            ? (response.type as keyof typeof RESPONSE_SCHEMAS)
            : undefined;
        console.log('[useShareWs.send] responseType=', responseType, 'for', type);

        const schema = responseType ? RESPONSE_SCHEMAS[responseType] : undefined;
        if (!schema) {
          console.error(
            '[useShareWs.send] missing validator for type=',
            responseType,
            'request=',
            type,
          );
          throw {
            code: 'PARSE_FAILED',
            kind: 'Malformed',
            detail: `no response schema for type=${String(responseType ?? 'unknown')} (request type=${type})`,
            message: 'Internal error: missing response validator',
          } satisfies ShareWsError;
        }

        const parse = schema.safeParse(response);
        if (!parse.success) {
          console.error(
            '[useShareWs.send] zod validation failed for',
            type,
            'issues=',
            parse.error.issues,
            'raw response=',
            response,
          );
          throw {
            code: 'PARSE_FAILED',
            kind: 'Malformed',
            detail: JSON.stringify(parse.error.issues),
            message: 'Received a malformed response from the host.',
          } satisfies ShareWsError;
        }

        console.log('[useShareWs.send] validated OK for', type);
        return parse.data as ShareRequestMap[typeof type]['result'];
      },

      subscribe(type, handler) {
        ensureOnShareEventSubscribed();
        let bucket = handlers.get(type);
        if (!bucket) {
          bucket = new Set();
          handlers.set(type, bucket);
        }
        const typedHandler = handler as (frame: unknown) => void;
        bucket.add(typedHandler);
        return () => {
          const b = handlers.get(type);
          if (!b) return;
          b.delete(typedHandler);
          if (b.size === 0) {
            handlers.delete(type);
          }
          maybeUnsubscribeOnShareEvent();
        };
      },
    }),
    [],
  );
}
