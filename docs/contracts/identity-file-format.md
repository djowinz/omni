# Identity File Format Contract

**Status:** Authoritative (Phase 0). Changes require umbrella update + version byte bump.

Two file formats:

1. `identity.key` — unencrypted local key, stored in the user's Omni config directory.
2. `.omniid` — passphrase-encrypted portable backup/restore file.

All multi-byte fields are little-endian unless otherwise stated. All hex in this doc is illustrative; canonical byte layout is authoritative.

## 1. `identity.key`

Purpose: persistent, OS-ACL-protected local store of the user's Ed25519 signing seed. Loss = identity rotation (new pubkey, TOFU mismatch for installers).

### Byte layout

```
offset  size  field                             notes
------  ----  --------------------------------  -------------------------------------
0       9     magic                              ASCII "OMNI-IDv1"
9       1     version                            0x01
10      32    seed                               Ed25519 32-byte seed
42      32    checksum                           HMAC-SHA-256(key=b"omni-id-local",
                                                              data=bytes[0..42])
74      —     EOF
```

Total length: exactly 74 bytes.

### Validation

1. Length MUST equal 74. Otherwise: `Corrupt("bad length")`.
2. Magic MUST equal `OMNI-IDv1`. Otherwise: `BadMagic`.
3. Version MUST equal `0x01`. Otherwise: `UnsupportedVersion(v)`.
4. Recompute `HMAC-SHA-256(b"omni-id-local", bytes[0..42])` and compare constant-time with bytes[42..74]. Mismatch: `BadChecksum`.
5. On success, construct `SigningKey::from_bytes(&bytes[10..42])`.

### Atomic write

Writers MUST:

1. Write to `identity.key.tmp` with mode `0o600` (POSIX) / current-user-only ACL (Windows).
2. `fsync` the file.
3. `rename` atomically over `identity.key`.
4. `fsync` the parent directory (POSIX).

Partial writes that leave `identity.key` truncated are treated as `Corrupt` and surface to the user rather than auto-recovering.

### Zeroization

Seed bytes MUST be wiped (`zeroize::Zeroize`) when dropped, on import failure, and after export encryption completes.

## 2. `.omniid` (encrypted backup)

Purpose: user-driven portable export. Encrypted with a passphrase so the file can be emailed, stored in a password manager, etc.

### Byte layout

```
offset  size  field                             notes
------  ----  --------------------------------  -------------------------------------
0       10    magic                              ASCII "OMNI-IDBAK"
10      1     version                            0x01
11      16    argon2id_salt                      cryptographically random
27      24    xchacha20poly1305_nonce            cryptographically random
51      32    argon2id_params_blob               see §2.1
83      N     ciphertext                         N = plaintext length (74 bytes)
83 + N  16    poly1305_tag                       XChaCha20-Poly1305 authenticator
```

Total length: `83 + N + 16` bytes = `83 + 74 + 16` = **173 bytes** for a valid backup.

### 2.1 `argon2id_params_blob` (32 bytes)

```
offset  size  field              value
------  ----  -----------------  -----------------------------------------
0       4     m_cost_kib         u32 LE; contract value: 65_536 (= 64 MiB)
4       4     t_cost             u32 LE; contract value: 3
8       4     p_cost             u32 LE; contract value: 4
12      4     output_len         u32 LE; contract value: 32
16      16    reserved           MUST be all-zero on write; ignored on read
```

Readers MUST reject `output_len != 32` (`Corrupt`). Readers MUST accept the contract values exactly; variations require a version bump.

### 2.2 Key derivation

```
key = Argon2id(password = passphrase_utf8,
               salt     = argon2id_salt,
               m        = m_cost_kib,
               t        = t_cost,
               p        = p_cost,
               length   = 32)
```

### 2.3 Encryption

```
plaintext  = full 74-byte `identity.key` content (including magic + local checksum)
aad        = magic (10 bytes) || version (1 byte) || argon2id_params_blob (32 bytes)
ciphertext, tag = XChaCha20-Poly1305.encrypt(key, nonce, plaintext, aad)
```

### 2.4 Validation on import

1. Length MUST equal 173. Otherwise: `Corrupt("bad length")`.
2. Magic MUST equal `OMNI-IDBAK`. Otherwise: `BadMagic`.
3. Version MUST equal `0x01`. Otherwise: `UnsupportedVersion(v)`.
4. Parse Argon2id params blob; reject on `output_len != 32`.
5. Derive key from passphrase.
6. Decrypt + authenticate. Tag mismatch → `BadPassphrase`.
7. Run §1 validation on the recovered 74-byte payload.

### 2.5 Atomic write & zeroization

Same atomic-write contract as `identity.key`. Derived key and plaintext buffers MUST be `zeroize`d after use.

## 3. Location defaults

| Platform | `identity.key` path                                                           |
| -------- | ----------------------------------------------------------------------------- |
| Windows  | `%APPDATA%\omni\identity.key`                                                 |
| macOS    | `~/Library/Application Support/omni/identity.key`                             |
| Linux    | `$XDG_CONFIG_HOME/omni/identity.key` (fallback `~/.config/omni/identity.key`) |

`.omniid` backup files live wherever the user chooses — no canonical location.

## 4. Versioning policy

Any change to byte layout, Argon2id params, HMAC key, or AAD MUST bump the version byte and is a contract change requiring an umbrella update. Readers MUST refuse unknown versions rather than attempting a best-effort parse.
