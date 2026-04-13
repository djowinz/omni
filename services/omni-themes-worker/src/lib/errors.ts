import type { ErrorBody, ErrorCode } from "../types";
import type { ContentfulStatusCode } from "hono/utils/http-status";

/**
 * Build a JSON error `Response` matching the contract envelope in
 * docs/superpowers/specs/contracts/worker-api.md §3. Usable in any context
 * (pure fn, no Hono `Context` needed) — for Hono routes, pass the result
 * to `return res;` or call `c.json(...)` directly using the same body shape.
 */
export function errorResponse(
  status: ContentfulStatusCode,
  code: ErrorCode,
  message: string,
  retryAfter?: number,
): Response {
  const body: ErrorBody = { error: { code, message } };
  if (retryAfter !== undefined) body.error.retry_after = retryAfter;
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

/** 501 skeleton response; route name is interpolated into the message. */
export function notImplemented(route: string): Response {
  return errorResponse(
    501,
    "NOT_IMPLEMENTED",
    `route ${route} is not implemented yet (sub-spec #007 skeleton)`,
  );
}
