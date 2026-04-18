# Host ⇄ Editor WebSocket Explorer Messages

**Status:** Authoritative (Phase 0). Changes require umbrella update.

Extends the existing `file_api` envelope:

```json
{ "id": "<uuid>", "type": "<message.type>", "params": { ... } }
```

Responses reuse the request `id` and use the matching `*Result`/`*Progress` type. Errors follow the `file_api` error envelope:

```json
{ "id": "<uuid>", "type": "error", "error": { "code": "…", "message": "…", "cause": "…" } }
```

All binary payloads are base64-encoded unless otherwise stated.

## Message index

| Request                  | Response(s)                                            |
| ------------------------ | ------------------------------------------------------ |
| `explorer.list`          | `explorer.listResult`                                  |
| `explorer.get`           | `explorer.getResult`                                   |
| `explorer.install`       | `explorer.installProgress`\*, `explorer.installResult` |
| `explorer.preview`       | `explorer.previewResult`                               |
| `explorer.cancelPreview` | `explorer.cancelPreviewResult`                         |
| `explorer.fork`          | `explorer.forkResult`                                  |
| `upload.pack`            | `upload.packResult`                                    |
| `upload.publish`         | `upload.publishProgress`\*, `upload.publishResult`     |
| `upload.update`          | `upload.updateResult`                                  |
| `upload.delete`          | `upload.deleteResult`                                  |
| `identity.show`          | `identity.showResult`                                  |
| `identity.backup`        | `identity.backupResult`                                |
| `identity.import`        | `identity.importResult`                                |
| `report.submit`          | `report.submitResult`                                  |

## Error codes

Carry-through from Worker codes (see `worker-api.md` §3) plus host-local:

| Code              | Meaning                                     |
| ----------------- | ------------------------------------------- |
| `OFFLINE`         | Host cannot reach Worker                    |
| `HOST_BUSY`       | Another install/publish in progress         |
| `SANDBOX_FAILED`  | Host resource sandbox refused the operation |
| `IDENTITY_LOCKED` | Identity file inaccessible                  |
| `PREVIEW_ACTIVE`  | Another preview is already live             |
| `NOT_FOUND_LOCAL` | Local workspace artifact missing            |

## `explorer.list`

**params**

```json
{
  "kind": "theme|bundle|all",
  "sort": "new|installs|name",
  "tags": ["dark"],
  "cursor": null,
  "limit": 25
}
```

**result**

```json
{
  "items": [
    /* Worker list item */
  ],
  "next_cursor": "…|null"
}
```

## `explorer.get`

**params** `{ "artifact_id": "…" }`
**result** `{ "artifact": /* Worker full metadata */ }`

Host MAY cache manifest and thumbnail in memory for 5 minutes.

## `explorer.install`

**params**

```json
{
  "artifact_id": "…",
  "target_workspace": "string",
  "overwrite": false,
  "expected_pubkey_hex": "<64-hex Ed25519 pubkey>|null"
}
```

The editor passes the full author pubkey (64-hex, 32 bytes decoded) — not the 6-byte fingerprint — because the shipped `InstallRequest.expected_pubkey: Option<PublicKey>` in `crates/host/src/share/install.rs` pins TOFU on the full key (no collision risk, strictly more precise). The editor has the full pubkey cached from every `GET /v1/list` / `GET /v1/artifact/:id` response (`author_pubkey` field per worker-api.md §4.3/§4.4). Fingerprints remain UI-facing only — shown in TOFU mismatch dialogs, fork confirmations, etc., via the `author_fingerprint_hex` field returned in list/get/install responses — but are never sent on install requests. Per invariant #21: shipped code is the contract oracle.
**progress** (zero or more before result)

```json
{ "phase": "download|verify|sanitize|write", "done": 1234, "total": 5678 }
```

**result**

```json
{
  "installed_path": "string",
  "content_hash": "…",
  "author_fingerprint_hex": "…",
  "tofu": "first_install|matched|mismatch",
  "warnings": ["…"]
}
```

On `tofu="mismatch"`, host returns `explorer.installResult` with error `TOFU_MISMATCH` and does NOT write to workspace.

## `explorer.preview`

Swaps the active overlay in place with a non-persistent preview.

**params** `{ "artifact_id": "…" }`
**result** `{ "preview_token": "uuid" }`

At most one preview active per host session. If one is active, error `PREVIEW_ACTIVE`.

## `explorer.cancelPreview`

**params** `{ "preview_token": "uuid" }`
**result** `{ "restored": true }`

## `explorer.fork`

Copies a remote bundle into a writable local workspace, clearing signature.

**params** `{ "artifact_id": "…", "target_name": "string" }`
**result** `{ "workspace_path": "…", "new_manifest": { /* manifest */ } }`

## `upload.pack`

Dry-run: compute canonical hash and sanitized size without uploading.

**params** `{ "workspace_path": "…" }`
**result**

```json
{
  "content_hash": "…",
  "compressed_size": 0,
  "uncompressed_size": 0,
  "manifest": {
    /* manifest */
  },
  "sanitize_report": {
    /* SanitizeReport */
  }
}
```

## `upload.publish`

**params**

```json
{ "workspace_path": "…", "visibility": "public", "bump": "patch|minor|major|none" }
```

**progress** `{ "phase": "pack|sanitize|upload", "done": 0, "total": 0 }`
**result**

```json
{ "artifact_id": "…", "content_hash": "…", "status": "created|deduplicated", "worker_url": "…" }
```

## `upload.update`

**params** `{ "artifact_id": "…", "workspace_path": "…", "bump": "patch|minor|major|none" }`
**result** same as `upload.publish`.

## `upload.delete`

**params** `{ "artifact_id": "…" }`
**result** `{ "deleted": true }`

## `identity.show`

**params** `{}`
**result**

```json
{
  "pubkey_hex": "…64-hex…",
  "fingerprint_hex": "aa11bb22cc33",
  "fingerprint_words": ["apple", "banana", "cobra"],
  "fingerprint_emoji": ["🦊", "🌲", "🚀", "🧊", "🌙", "⚡"],
  "created_at": 0
}
```

## `identity.backup`

**params** `{ "passphrase": "string" }`
**result** `{ "encrypted_bytes_b64": "…" }`

## `identity.import`

**params** `{ "encrypted_bytes_b64": "…", "passphrase": "string",
            "overwrite_existing": false }`
**result** `{ "pubkey_hex": "…", "fingerprint_hex": "…" }`

## `report.submit`

**params** `{ "artifact_id": "…", "category": "illegal|malware|impersonation|nsfw|other", "note": "string" }`
**result** `{ "report_id": "…", "status": "received" }`
