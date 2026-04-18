//! WASM bindings for omni-bundle. Enabled only under the `wasm` feature.
//!
//! Thin binding layer over the native public API:
//! - `canonicalHash(manifest) -> Uint8Array`
//! - `unpackManifest(bytes, limits) -> Manifest JSON` (invariant #19b fast path)
//! - `unpack(bytes, limits) -> WasmUnpackHandle` with `next() -> {path, bytes} | null`
//! - `pack(manifest, files) -> Uint8Array`
//!
//! Design notes:
//! - `Unpack<'a>` borrows from input bytes; WASM hands us a `&[u8]` that does
//!   not outlive the call, so we materialize via `into_map()` and yield entries
//!   one at a time. `BundleLimits` already caps total size, so peak memory is
//!   bounded at the Worker level.

use std::collections::BTreeMap;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::{
    canonical_hash as canonical_hash_native, pack as pack_native, unpack as unpack_native,
    unpack_manifest as unpack_manifest_native, BundleLimits, Manifest,
};

fn to_js_err<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Accept either a JS `Map<string, Uint8Array>` or a plain object whose values are
/// `Uint8Array`. Returns a Rust `BTreeMap<String, Vec<u8>>`.
pub(crate) fn files_from_js(v: &JsValue) -> Result<BTreeMap<String, Vec<u8>>, JsValue> {
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
            let value =
                js_sys::Reflect::get(obj, &key_js).map_err(|_| JsValue::from_str("reflect get"))?;
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

#[wasm_bindgen(js_name = "canonicalHash")]
pub fn canonical_hash_wasm(manifest_js: JsValue) -> Result<Vec<u8>, JsValue> {
    let m: Manifest = serde_wasm_bindgen::from_value(manifest_js).map_err(to_js_err)?;
    let files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    Ok(canonical_hash_native(&m, &files).to_vec())
}

#[wasm_bindgen(js_name = "unpackManifest")]
pub fn unpack_manifest_wasm(bytes: &[u8], limits_js: JsValue) -> Result<JsValue, JsValue> {
    let limits = limits_from_js(limits_js)?;
    let manifest = unpack_manifest_native(bytes, &limits).map_err(to_js_err)?;
    serde_wasm_bindgen::to_value(&manifest).map_err(to_js_err)
}

/// Handle returned from `unpack`. JS calls `next()` until it returns `null`.
#[wasm_bindgen]
pub struct WasmUnpackHandle {
    manifest: Manifest,
    remaining: std::vec::IntoIter<(String, Vec<u8>)>,
}

#[wasm_bindgen]
impl WasmUnpackHandle {
    #[wasm_bindgen(js_name = "manifest")]
    pub fn manifest(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.manifest).map_err(to_js_err)
    }

    /// Yields `{ path: string, bytes: Uint8Array }` or `null` when exhausted.
    #[wasm_bindgen(js_name = "next")]
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

#[wasm_bindgen(js_name = "unpack")]
pub fn unpack_wasm(bytes: &[u8], limits_js: JsValue) -> Result<WasmUnpackHandle, JsValue> {
    let limits = limits_from_js(limits_js)?;
    let unpack = unpack_native(bytes, &limits).map_err(to_js_err)?;
    let (manifest, files) = unpack.into_map().map_err(to_js_err)?;
    let remaining: Vec<(String, Vec<u8>)> = files.into_iter().collect();
    Ok(WasmUnpackHandle {
        manifest,
        remaining: remaining.into_iter(),
    })
}

#[wasm_bindgen(js_name = "pack")]
pub fn pack_wasm(
    manifest_js: JsValue,
    files_js: JsValue,
    limits_js: JsValue,
) -> Result<Vec<u8>, JsValue> {
    let manifest: Manifest = serde_wasm_bindgen::from_value(manifest_js).map_err(to_js_err)?;
    // files_js is expected as a JS `Map<string, Uint8Array>`. Iterate it explicitly
    // to avoid serde bouncing Uint8Array through an intermediate JSON array.
    let files = files_from_js(&files_js)?;
    let limits = limits_from_js(limits_js)?;
    pack_native(&manifest, &files, &limits).map_err(to_js_err)
}
