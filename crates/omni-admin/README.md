# omni-admin

Moderation CLI for the Omni theme-sharing Worker. Reactive moderation: users file reports, moderators review and act from this CLI. All actions authenticate via an Ed25519 JWS signed with a dedicated admin keypair — the same `omni-identity::Keypair::sign_jws` path host bundle uploads use.

## Setup (first time)

1. Generate the admin keypair on a moderator workstation:

       omni-admin keygen --output ./admin-identity.key

   The command prints the hex-encoded public key.

2. Add the hex pubkey to the Worker's `OMNI_ADMIN_PUBKEYS` env var (comma-separated list; case-insensitive; trimmed) and redeploy. Multiple moderators = multiple entries.

3. Move `admin-identity.key` to its permanent location:

   - **Linux/macOS:** `~/.config/omni/admin-identity.key` (mode `600`)
   - **Windows:** `%APPDATA%\Omni\admin-identity.key` with an ACL locked to your user:

         icacls "%APPDATA%\Omni\admin-identity.key" /inheritance:r /grant:r %USERNAME%:R

   `omni-admin` refuses to run if permissions are overbroad — standard hygiene, same pattern SSH (`StrictModes`), GnuPG (`~/.gnupg` 0700), and `kubectl` enforce.

## Global flags

| Flag | Effect |
|---|---|
| `--key-file <path>` | Override the default key path (also `OMNI_ADMIN_KEY_FILE` env). |
| `--worker-url <url>` | Worker base URL (default `https://themes.omni.prod/`; also `OMNI_ADMIN_WORKER_URL` env). |
| `--yes` | Skip interactive confirmations. Scripts only. |
| `--json` | Emit machine-readable JSON instead of pretty text. |

## Daily use

    omni-admin review               # interactive loop over pending reports
    omni-admin reports list         # list pending reports
    omni-admin reports show <id>    # inspect a specific report
    omni-admin artifact remove <id> --reason "copyright"
    omni-admin pubkey  ban   <hex>  --reason "malware" [--confirm-cascade]
    omni-admin device  ban   <hex>  --reason "abuse"
    omni-admin vocab   add   <tag>
    omni-admin limits  set   --max-bundle-compressed 5242880 [--force]
    omni-admin stats

`review` is the primary moderator interface. It walks pending reports one at a time, renders a summary + thumbnail preview (downloaded to a tempfile and opened with the OS default viewer), and prompts `[k]eep / [r]emove / [b]an author / [s]kip / [q]uit`. `[b]an author` requires a second confirmation unless `--yes`. Ctrl-C between prompts is safe — each action is a single atomic Worker request.

Every state-changing action appends one line to `~/.omni-admin/audit.log`:

    2026-04-14T23:32:11Z REMOVE artifact=abc reason="copyright"
    2026-04-14T23:35:22Z BAN pubkey=aa... reason="malware" cascade_count=3 cascade_errors=0
    2026-04-14T23:40:05Z VOCAB add=retrowave version_after=5

The log is local-only and forensic — the Worker's KV/D1 state plus its access logs are the authoritative record.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Generic error (bad args, client-side failures) |
| `2` | `Admin.*` (not moderator, bad tag, would-orphan, bad value, no-op) |
| `3` | `Auth.*` (signature, stale timestamp, mismatched method/path) |
| `4` | `Malformed` / `Integrity` |
| `5` | `Io` |
| `6` | `Quota` |

`--json` mode emits the full `{ error: { code, kind, detail, message } }` envelope on failure so scripts can branch on `kind` without parsing stderr.

## Testing notes

Integration tests use `wiremock` to stand in for the Worker. The env var `OMNI_ADMIN_AUDIT_DIR` overrides the default `~/.omni-admin/` location so tests can redirect the audit log into a per-test tempdir — on Windows `directories::BaseDirs` consults `SHGetKnownFolderPath` which does not honor a re-exported `USERPROFILE`, hence the explicit override. This env var is documented for test use only; production deployments leave it unset.
