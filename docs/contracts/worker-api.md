# Worker HTTP API Contract

**Status:** Authoritative (Phase 0). Changes require umbrella update.
**Base URL:** `https://worker.omni.<env>/` (env ∈ {dev, staging, prod}).
**Transport:** HTTPS only. TLS 1.3. HTTP/2.
**Serialization:** JSON (UTF-8) unless otherwise noted.

## 1. Common headers

Every authenticated request MUST include:

| Header                    | Meaning                            | Format            |
| ------------------------- | ---------------------------------- | ----------------- |
| `X-Omni-Version`          | Client app semver                  | `^\d+\.\d+\.\d+$` |
| `X-Omni-DF`               | Device fingerprint                 | base64(32 bytes)  |
| `X-Omni-Pubkey`           | Ed25519 public key                 | base64(32 bytes)  |
| `X-Omni-Timestamp`        | Unix seconds at time of request    | integer as string |
| `X-Omni-Signature`        | Signature over canonical string    | base64(64 bytes)  |
| `X-Omni-Sanitize-Version` | Client's sanitize pipeline version | integer as string |

Unauthenticated endpoints (`GET /v1/list`, `GET /v1/artifact/:id`, `GET /v1/download/:id`) MAY omit pubkey/signature but MUST still send version + DF for rate limiting.

## 2. Canonical string for signing

```
<METHOD>\n<path>\n<sorted_query>\n<pubkey_hex>\n<df_hex>\n<sanitize_version>\n<timestamp>\n<sha256_hex(body)>
```

- `METHOD` — uppercase (`GET`, `POST`, …)
- `path` — URL path, no query, no trailing slash
- `sorted_query` — query pairs sorted by key, URL-encoded, joined with `&`; empty string if none
- `pubkey_hex` / `df_hex` — lowercase hex
- `sanitize_version` — decimal integer
- `timestamp` — decimal integer (unix seconds); server rejects if drift > 300s
- `sha256_hex(body)` — lowercase hex of request body SHA-256; empty body → `sha256("")`

Signature = `Ed25519.sign(seed_of_pubkey, canonical_string.as_bytes())`.

## 3. Error shape

All 4xx/5xx responses:

```json
{ "error": { "code": "STRING_CODE", "message": "human readable", "retry_after": 60 } }
```

`retry_after` present only on 429.

### Error codes

| Code                  | HTTP | Meaning                                    |
| --------------------- | ---- | ------------------------------------------ |
| `BAD_SIGNATURE`       | 401  | Signature invalid or missing               |
| `STALE_TIMESTAMP`     | 401  | `X-Omni-Timestamp` drift > 300s            |
| `UNKNOWN_PUBKEY`      | 403  | Pubkey on denylist                         |
| `RATE_LIMITED`        | 429  | See `retry_after`                          |
| `TURNSTILE_REQUIRED`  | 428  | Must solve captcha; body contains site key |
| `BAD_REQUEST`         | 400  | Shape violation                            |
| `MANIFEST_INVALID`    | 400  | Manifest JSON failed schema                |
| `SIZE_EXCEEDED`       | 413  | Body too large                             |
| `UNSUPPORTED_VERSION` | 426  | Sanitize version too old                   |
| `NOT_FOUND`           | 404  | Artifact id unknown                        |
| `CONFLICT`            | 409  | Name collision under this pubkey           |
| `FORBIDDEN`           | 403  | Not owner of target                        |
| `TOMBSTONED`          | 410  | Content removed by moderation              |
| `SERVER_ERROR`        | 500  | Unexpected                                 |

## 4. Endpoints

### 4.1 `POST /v1/upload`

Auth: required.
Body: `multipart/form-data` with parts:

- `manifest` — JSON (`application/json`), max 32 KiB
- `bundle` — bytes (`application/octet-stream`), ZIP for bundles or raw CSS for themes
- `thumbnail` — PNG (`image/png`), max 256 KiB

Server validates manifest against `bundle-manifest.schema.json`, runs full sanitize pipeline, computes content hash, dedups, records author pubkey on first upload.

Response `200` (new) or `200` (dedup):

```json
{
  "artifact_id": "b7e…",
  "content_hash": "…64-hex…",
  "r2_url": "https://…",
  "thumbnail_url": "https://…",
  "created_at": 1760000000,
  "status": "created"
}
```

`status` is `"created"` or `"deduplicated"`.

Errors: `MANIFEST_INVALID`, `SIZE_EXCEEDED`, `RATE_LIMITED`, `TURNSTILE_REQUIRED`, `CONFLICT`.

### 4.2 `GET /v1/download/:artifact_id`

Auth: optional.
Returns raw sanitized bytes with `Content-Type: application/octet-stream` (bundles) or `text/css` (themes). Increments install counter (DF-rate-limited).

Headers in response:

- `X-Omni-Content-Hash` — content hash
- `X-Omni-Author-Pubkey` — uploader pubkey, hex
- `X-Omni-Signature` — Ed25519 over content bytes
- `X-Omni-Manifest` — base64 of manifest JSON

Errors: `NOT_FOUND`, `TOMBSTONED`, `RATE_LIMITED`.

### 4.3 `GET /v1/list`

Auth: optional.
Query:

- `kind` — `theme | bundle | all` (default `all`)
- `sort` — `new | installs | name` (default `new`)
- `tag` — any value from the tag vocabulary; repeatable
- `cursor` — opaque pagination cursor (see §5)
- `limit` — 1…100 (default 25)
- `author_pubkey` — 64-hex string; optional. When provided, filters results to a single author's artifacts. Invalid hex returns `400 BAD_REQUEST` with `kind: "Malformed"`.

Response:

```json
{
  "items": [
    {
      "artifact_id": "…",
      "name": "…",
      "kind": "bundle",
      "tags": ["dark", "gaming"],
      "installs": 42,
      "updated_at": 1760000000,
      "author_pubkey": "…64-hex…",
      "author_fingerprint_hex": "aa11bb22cc33",
      "thumbnail_url": "…",
      "content_hash": "…"
    }
  ],
  "next_cursor": "eyJ0IjoxNzYwLCJpIjoiYWJjIn0"
}
```

`next_cursor` omitted when no more results.

### 4.4 `GET /v1/artifact/:id`

Full metadata including manifest and report counts.

Response:

```json
{
  "artifact_id": "…",
  "kind": "bundle",
  "manifest": {
    /* exact manifest JSON */
  },
  "content_hash": "…",
  "r2_url": "…",
  "thumbnail_url": "…",
  "author_pubkey": "…",
  "author_fingerprint_hex": "…",
  "installs": 0,
  "reports": 0,
  "created_at": 0,
  "updated_at": 0,
  "status": "live"
}
```

`status` ∈ `live | tombstoned | moderation_hold`.

### 4.5 `PATCH /v1/artifact/:id`

Auth: required, signing pubkey MUST match original author pubkey.
Body: multipart, same parts as upload; omitted parts are unchanged.
Response: same as `POST /v1/upload`.
Errors: `FORBIDDEN`, `NOT_FOUND`.

### 4.6 `DELETE /v1/artifact/:id`

Auth: required (owner). Soft-deletes.
Response: `204 No Content`.
Errors: `FORBIDDEN`, `NOT_FOUND`.

### 4.7 `POST /v1/report`

Auth: required.
Body:

```json
{
  "artifact_id": "…",
  "category": "illegal|malware|impersonation|nsfw|other",
  "note": "≤500 chars"
}
```

Response:

```json
{ "report_id": "…", "status": "received" }
```

### 4.8 `GET /v1/me/gallery`

Auth: required. Paginated list filtered to authoring pubkey.
Same shape as `/v1/list`.

### 4.9 `GET /v1/config/vocab`

Auth: optional. Unauthenticated reads return the current tag vocabulary. Added per retro-005 D6 — clients fetch-and-cache instead of compiling in; admins edit via `/v1/admin/*` (#012).

Response:

```json
{
  "tags": ["dark", "light", "minimal", "gaming", "..."],
  "version": 1
}
```

Client caching: tolerable for 24h. `version` increments on admin edit; clients refetch on mismatch.

Errors: none in happy path. `SERVER_ERROR` only on KV read failure.

### 4.10 `GET /v1/config/limits`

Auth: optional. Returns the current bundle-size policy. Added per retro-005 D7 — `max_bundle_compressed` also serves as the HTTP request-body cap for uploads. Security-level constants (path depth, compression ratio, path length) are NOT returned here; those stay compile-time in `omni-bundle`.

Response:

```json
{
  "max_bundle_compressed": 5242880,
  "max_bundle_uncompressed": 10485760,
  "max_entries": 32,
  "version": 1,
  "updated_at": 1760000000
}
```

Client caching: same as `/v1/config/vocab`.

Errors: none in happy path. `SERVER_ERROR` only on KV read failure.

## 5. Pagination cursor

Cursors are opaque base64url of JSON `{ "t": <updated_at>, "i": "<artifact_id>" }`. Clients MUST treat as opaque; server MAY change format without notice.
