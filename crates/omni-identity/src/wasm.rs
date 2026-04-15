//! WASM bindings for omni-identity. Enabled only under the `wasm` feature.
//!
//! The native crate uses `jsonwebtoken` → `ring` for JWS, but `ring` does not
//! cross-compile to `wasm32-unknown-unknown` without a C toolchain (clang).
//! For the Worker WASM build we hand-roll JWS compact EdDSA via
//! `ed25519-dalek` directly. Byte-format equivalence with the native path is
//! verified by the canonical-hash parity and JWS roundtrip tests in
//! `services/omni-themes-worker/test/`.
//!
//! Exports:
//! - `verifyJws(token, pubkey) -> claims`
//! - `signJws(claims, privKey) -> compactJws`
//! - `unpackSignedBundle(bytes, limits) -> WasmSignedBundleHandle`
//! - `packSignedBundle(manifest, files, privKey, limits) -> Uint8Array`

use std::collections::BTreeMap;

use base64::Engine;
use ed25519_dalek::{Verifier, VerifyingKey};
use omni_bundle::{
    canonical_hash, pack as bundle_pack, unpack as bundle_unpack, BundleLimits, FileEntry, Manifest,
};
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::fingerprint::PublicKey;

const SIGNATURE_FILENAME: &str = "signature.jws";
const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

fn to_js_err<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn limits_from_js(limits_js: JsValue) -> Result<BundleLimits, JsValue> {
    #[derive(serde::Deserialize)]
    struct LimitsShim {
        max_bundle_compressed: u64,
        max_bundle_uncompressed: u64,
        max_entries: usize,
    }
    if limits_js.is_undefined() || limits_js.is_null() {
        return Ok(BundleLimits::DEFAULT);
    }
    let s: LimitsShim = serde_wasm_bindgen::from_value(limits_js).map_err(to_js_err)?;
    Ok(BundleLimits {
        max_bundle_compressed: s.max_bundle_compressed,
        max_bundle_uncompressed: s.max_bundle_uncompressed,
        max_entries: s.max_entries,
    })
}

fn files_from_js(v: &JsValue) -> Result<BTreeMap<String, Vec<u8>>, JsValue> {
    let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    if v.is_instance_of::<js_sys::Map>() {
        let map: &js_sys::Map = v.unchecked_ref();
        let entries = map.entries();
        loop {
            let next = entries.next().map_err(|_| JsValue::from_str("iter next"))?;
            if js_sys::Reflect::get(&next, &JsValue::from_str("done"))
                .map_err(|_| JsValue::from_str("done"))?
                .as_bool()
                .unwrap_or(false)
            {
                break;
            }
            let value = js_sys::Reflect::get(&next, &JsValue::from_str("value"))
                .map_err(|_| JsValue::from_str("value"))?;
            let arr: js_sys::Array = value.dyn_into().map_err(|_| JsValue::from_str("entry"))?;
            let key = arr
                .get(0)
                .as_string()
                .ok_or_else(|| JsValue::from_str("map key must be string"))?;
            let bytes: js_sys::Uint8Array = arr
                .get(1)
                .dyn_into()
                .map_err(|_| JsValue::from_str("map value must be Uint8Array"))?;
            out.insert(key, bytes.to_vec());
        }
    } else if v.is_object() {
        let obj: &js_sys::Object = v.unchecked_ref();
        let keys = js_sys::Object::keys(obj);
        for i in 0..keys.length() {
            let key_js = keys.get(i);
            let key = key_js
                .as_string()
                .ok_or_else(|| JsValue::from_str("object key must be string"))?;
            let value = js_sys::Reflect::get(obj, &key_js)
                .map_err(|_| JsValue::from_str("reflect get"))?;
            let bytes: js_sys::Uint8Array = value
                .dyn_into()
                .map_err(|_| JsValue::from_str("object value must be Uint8Array"))?;
            out.insert(key, bytes.to_vec());
        }
    } else {
        return Err(JsValue::from_str(
            "files must be a Map<string, Uint8Array> or plain object of Uint8Array",
        ));
    }
    Ok(out)
}

fn seed_from_slice(priv_key: &[u8]) -> Result<[u8; 32], JsValue> {
    if priv_key.len() != 32 {
        return Err(JsValue::from_str(
            "private key must be 32 bytes (Ed25519 seed)",
        ));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(priv_key);
    Ok(seed)
}

fn pubkey_from_slice(pubkey: &[u8]) -> Result<PublicKey, JsValue> {
    if pubkey.len() != 32 {
        return Err(JsValue::from_str("public key must be 32 bytes"));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(pubkey);
    Ok(PublicKey(arr))
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

// ---------------------------------------------------------------------------
// JWS compact EdDSA with embedded OKP jwk header — byte-equivalent to the
// native path's jsonwebtoken output (header key order is `alg, typ, jwk`).
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct OkpJwk {
    crv: String,
    kty: String,
    x: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JwsHeader {
    alg: String,
    typ: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    jwk: Option<OkpJwk>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SignaturePayload {
    canonical_hash_hex: String,
}

fn sign_jws_compact(
    claims: &serde_json::Value,
    seed: &[u8; 32],
    embed_jwk: bool,
) -> Result<String, JsValue> {
    // Delegates to the unconditionally-compiled pure-Rust core so the native
    // byte-parity regression test in `tests/jws_native_wasm_parity.rs` can
    // exercise the exact same signing bytes this WASM path produces.
    crate::wasm_jws_core::sign_jws_compact_core(claims, seed, embed_jwk).map_err(to_js_err)
}

fn jws_parts(token: &str) -> Result<(&str, &str, &str), JsValue> {
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(JsValue::from_str("jws: not a 3-part compact token"));
    }
    Ok((parts[0], parts[1], parts[2]))
}

fn verify_jws_with_pubkey(token: &str, pubkey: &[u8; 32]) -> Result<serde_json::Value, JsValue> {
    let (h_b64, p_b64, s_b64) = jws_parts(token)?;
    let signing_input = format!("{h_b64}.{p_b64}");
    let sig_bytes = B64
        .decode(s_b64)
        .map_err(|e| JsValue::from_str(&format!("jws sig b64: {e}")))?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| JsValue::from_str("jws sig must be 64 bytes"))?;
    let vk = VerifyingKey::from_bytes(pubkey)
        .map_err(|e| JsValue::from_str(&format!("jws pubkey: {e}")))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(signing_input.as_bytes(), &sig)
        .map_err(|e| JsValue::from_str(&format!("jws verify: {e}")))?;

    let header_bytes = B64
        .decode(h_b64)
        .map_err(|e| JsValue::from_str(&format!("jws header b64: {e}")))?;
    let header: JwsHeader = serde_json::from_slice(&header_bytes)
        .map_err(|e| JsValue::from_str(&format!("jws header json: {e}")))?;
    if header.alg != "EdDSA" {
        return Err(JsValue::from_str(&format!(
            "jws alg must be EdDSA, got {}",
            header.alg
        )));
    }
    let payload_bytes = B64
        .decode(p_b64)
        .map_err(|e| JsValue::from_str(&format!("jws payload b64: {e}")))?;
    serde_json::from_slice::<serde_json::Value>(&payload_bytes)
        .map_err(|e| JsValue::from_str(&format!("jws payload json: {e}")))
}

fn extract_jws_pubkey(token: &str) -> Result<[u8; 32], JsValue> {
    let (h_b64, _, _) = jws_parts(token)?;
    let header_bytes = B64
        .decode(h_b64)
        .map_err(|e| JsValue::from_str(&format!("jws header b64: {e}")))?;
    let header: JwsHeader = serde_json::from_slice(&header_bytes)
        .map_err(|e| JsValue::from_str(&format!("jws header json: {e}")))?;
    let jwk = header
        .jwk
        .ok_or_else(|| JsValue::from_str("jws header missing jwk"))?;
    if jwk.kty != "OKP" || jwk.crv != "Ed25519" {
        return Err(JsValue::from_str(&format!(
            "jwk must be OKP/Ed25519, got {}/{}",
            jwk.kty, jwk.crv
        )));
    }
    let x_bytes = B64
        .decode(&jwk.x)
        .map_err(|e| JsValue::from_str(&format!("jwk.x b64: {e}")))?;
    x_bytes
        .try_into()
        .map_err(|_| JsValue::from_str("jwk.x must be 32 bytes"))
}

// ---------------------------------------------------------------------------
// Public wasm-bindgen surface
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = "verifyJws")]
pub fn verify_jws_wasm(token: &str, pubkey: &[u8]) -> Result<JsValue, JsValue> {
    let pk = pubkey_from_slice(pubkey)?;
    let claims = verify_jws_with_pubkey(token, &pk.0)?;
    serde_wasm_bindgen::to_value(&claims).map_err(to_js_err)
}

#[wasm_bindgen(js_name = "signJws")]
pub fn sign_jws_wasm(claims_js: JsValue, priv_key: &[u8]) -> Result<String, JsValue> {
    let seed = seed_from_slice(priv_key)?;
    let claims: serde_json::Value = serde_wasm_bindgen::from_value(claims_js).map_err(to_js_err)?;
    sign_jws_compact(&claims, &seed, false)
}

// ---------------------------------------------------------------------------
// Signed bundle pack / unpack (inline, wasm-only)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct WasmSignedBundleHandle {
    manifest: Manifest,
    author_pubkey: [u8; 32],
    remaining: std::vec::IntoIter<(String, Vec<u8>)>,
}

#[wasm_bindgen]
impl WasmSignedBundleHandle {
    #[wasm_bindgen(js_name = "manifest")]
    pub fn manifest(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.manifest).map_err(to_js_err)
    }

    #[wasm_bindgen(js_name = "authorPubkey")]
    pub fn author_pubkey(&self) -> Vec<u8> {
        self.author_pubkey.to_vec()
    }

    #[wasm_bindgen(js_name = "nextFile")]
    pub fn next_file(&mut self) -> Result<JsValue, JsValue> {
        let Some((path, bytes)) = self.remaining.next() else {
            return Ok(JsValue::NULL);
        };
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &JsValue::from_str("path"), &JsValue::from_str(&path))
            .map_err(|_| JsValue::from_str("set path"))?;
        let arr = js_sys::Uint8Array::from(bytes.as_slice());
        js_sys::Reflect::set(&obj, &JsValue::from_str("bytes"), &arr.into())
            .map_err(|_| JsValue::from_str("set bytes"))?;
        Ok(obj.into())
    }
}

#[wasm_bindgen(js_name = "packSignedBundle")]
pub fn pack_signed_bundle_wasm(
    manifest_js: JsValue,
    files_js: JsValue,
    priv_key: &[u8],
    limits_js: JsValue,
) -> Result<Vec<u8>, JsValue> {
    let manifest: Manifest = serde_wasm_bindgen::from_value(manifest_js).map_err(to_js_err)?;
    let files = files_from_js(&files_js)?;
    let seed = seed_from_slice(priv_key)?;
    let limits = limits_from_js(limits_js)?;

    let mut pre_sig_manifest = manifest.clone();
    pre_sig_manifest.files.sort_by(|a, b| a.path.cmp(&b.path));
    let digest = canonical_hash(&pre_sig_manifest, &files);
    let payload = SignaturePayload {
        canonical_hash_hex: hex::encode(digest),
    };
    let claims_json = serde_json::to_value(&payload).map_err(to_js_err)?;
    let jws_compact = sign_jws_compact(&claims_json, &seed, true)?;

    let mut amended_manifest = pre_sig_manifest;
    amended_manifest.files.push(FileEntry {
        path: SIGNATURE_FILENAME.into(),
        sha256: sha256_bytes(jws_compact.as_bytes()),
    });
    amended_manifest.files.sort_by(|a, b| a.path.cmp(&b.path));

    let mut amended_files = files;
    amended_files.insert(SIGNATURE_FILENAME.into(), jws_compact.into_bytes());

    bundle_pack(&amended_manifest, &amended_files, &limits).map_err(to_js_err)
}

#[wasm_bindgen(js_name = "unpackSignedBundle")]
pub fn unpack_signed_bundle_wasm(
    bytes: &[u8],
    limits_js: JsValue,
) -> Result<WasmSignedBundleHandle, JsValue> {
    let limits = limits_from_js(limits_js)?;
    let unpack = bundle_unpack(bytes, &limits).map_err(to_js_err)?;
    let (amended_manifest, mut files_map) = unpack.into_map().map_err(to_js_err)?;

    let jws_bytes = files_map
        .remove(SIGNATURE_FILENAME)
        .ok_or_else(|| JsValue::from_str("missing signature.jws"))?;
    let jws_str = std::str::from_utf8(&jws_bytes)
        .map_err(|e| JsValue::from_str(&format!("non-utf8 jws: {e}")))?;

    let mut original_manifest = amended_manifest;
    original_manifest
        .files
        .retain(|f| f.path != SIGNATURE_FILENAME);

    let expected_hex = hex::encode(canonical_hash(&original_manifest, &BTreeMap::new()));

    let author_pubkey = extract_jws_pubkey(jws_str)?;
    let claims = verify_jws_with_pubkey(jws_str, &author_pubkey)?;
    let payload: SignaturePayload = serde_json::from_value(claims).map_err(to_js_err)?;
    if payload.canonical_hash_hex != expected_hex {
        return Err(JsValue::from_str(&format!(
            "canonical_hash mismatch: signed {}, computed {}",
            payload.canonical_hash_hex, expected_hex
        )));
    }

    let remaining: Vec<(String, Vec<u8>)> = files_map.into_iter().collect();
    Ok(WasmSignedBundleHandle {
        manifest: original_manifest,
        author_pubkey,
        remaining: remaining.into_iter(),
    })
}
