/**
 * useShareWs — typed WebSocket client for the Omni Share Hub message surface.
 *
 * ## send(type, params): Promise<result>
 * Request-response path. Routes through `window.omni.sendMessage({ type, ...params })`
 * (an `ipcRenderer.invoke` under the hood). Response is Zod-validated via
 * `ShareResponseSchemas[type]`. On parse failure, rejects with `ShareWsError`
 * `{ code: "PARSE_FAILED", ... }`. On host error envelope, rejects with the
 * D-004-J `{ code, kind, detail, message }` shape (Zod-validated against
 * `ShareErrorFrameSchema`).
 *
 * ## subscribe(type, handler): () => void
 * Subscribes to unsolicited *Progress* / preview-result streaming frames. Uses
 * the `window.omni.onShareEvent` IPC channel (see `apps/desktop/main/preload.ts`
 * + the SHARE_EVENT_TYPES forwarding in `main.ts`). A single module-level
 * subscription to `onShareEvent` is shared across all hook consumers; incoming
 * frames are dispatched to handlers keyed by `frame.type`. Returns an unsubscribe
 * that removes the handler (and the underlying onShareEvent listener when no
 * handlers remain).
 *
 * ## Design decision: IPC routing
 * Request-response could have gone through an extension of the responseTypes
 * map in `main.ts:162-177`, but that would require pairing every share message
 * request with its response type and synthesizing invoke() semantics for
 * streaming frames. The simpler design uses the existing `ws-message` invoke
 * path (unchanged, works today) for request-response and a new `share:event`
 * channel for unsolicited frames only. Documented here so Wave-3b implementers
 * know not to add share-specific responseTypes entries.
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
        const response = await window.omni!.sendMessage({ type, ...params });

        // First check for error envelope.
        const errParse = ShareErrorFrameSchema.safeParse(response);
        if (errParse.success) {
          throw errParse.data.error as ShareWsError;
        }

        // Resolve the response schema by the `type` field on the returned frame.
        // The host always includes `type` on response frames (e.g. 'explorer.listResult').
        const responseType =
          typeof response === 'object' && response !== null && typeof response.type === 'string'
            ? (response.type as keyof typeof RESPONSE_SCHEMAS)
            : undefined;

        const schema = responseType ? RESPONSE_SCHEMAS[responseType] : undefined;
        if (!schema) {
          throw {
            code: 'PARSE_FAILED',
            kind: 'Malformed',
            detail: `no response schema for type=${String(responseType ?? 'unknown')} (request type=${type})`,
            message: 'Internal error: missing response validator',
          } satisfies ShareWsError;
        }

        const parse = schema.safeParse(response);
        if (!parse.success) {
          throw {
            code: 'PARSE_FAILED',
            kind: 'Malformed',
            detail: JSON.stringify(parse.error.issues),
            message: 'Received a malformed response from the host.',
          } satisfies ShareWsError;
        }

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
