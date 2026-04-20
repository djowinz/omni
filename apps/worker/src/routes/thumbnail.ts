/**
 * GET /v1/thumbnail/:hash — unauthenticated PNG serving.
 *
 * Thumbnails are content-addressed by SHA-256 of the PNG bytes (captured
 * by the host at upload time; see `crates/host/src/share/thumbnail`).
 * The hash is the R2 key, so no D1 lookup is needed — fetch the blob
 * directly.
 *
 * No rate limit: thumbnails are immutable, content-addressed, and served
 * from R2 (edge-cacheable). Abuse protection lives upstream at the list/
 * gallery level that hands out the hashes.
 */
import { Hono } from 'hono';
import type { AppEnv } from '../types';
import { errorFromKind } from '../lib/errors';

const app = new Hono<AppEnv>();

app.get('/:hash', async (c) => {
  const env = c.env;
  const hash = c.req.param('hash');

  // Basic shape check — thumbnail hashes are 64-char lowercase hex.
  if (!/^[0-9a-f]{64}$/.test(hash)) {
    return errorFromKind('Malformed', 'BadRequest', 'invalid thumbnail hash');
  }

  const obj = await env.BLOBS.get(`thumbnails/${hash}.png`);
  if (!obj) return errorFromKind('Malformed', 'NotFound', 'thumbnail not found');

  const bytes = await obj.arrayBuffer();
  return new Response(bytes, {
    status: 200,
    headers: {
      'content-type': 'image/png',
      'Cache-Control': 'public, max-age=31536000, immutable',
    },
  });
});

export default app;
