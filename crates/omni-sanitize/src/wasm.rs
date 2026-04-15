//! WASM bindings for omni-sanitize. Enabled only under the `wasm` feature.
//!
//! Exports:
//! - `sanitizeTheme(bytes) -> { sanitized: Uint8Array, report: SanitizeReport }`
//! - `sanitizeBundle(manifest, files) -> { sanitized: {path: Uint8Array, …}, report }`
//! - `rejectExecutableMagic(bytes) -> { ok: bool, prefixHex?: string }` (invariant #19c)

use std::collections::BTreeMap;

use omni_bundle::Manifest;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::magic::reject_executable_magic;
use crate::{sanitize_bundle as sanitize_bundle_native, sanitize_theme as sanitize_theme_native};

fn to_js_err<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
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

fn files_to_js(files: &BTreeMap<String, Vec<u8>>) -> Result<JsValue, JsValue> {
    let obj = js_sys::Object::new();
    for (path, bytes) in files {
        let arr = js_sys::Uint8Array::from(bytes.as_slice());
        js_sys::Reflect::set(&obj, &JsValue::from_str(path), &arr.into())
            .map_err(|_| JsValue::from_str("set file"))?;
    }
    Ok(obj.into())
}

#[wasm_bindgen(js_name = "sanitizeTheme")]
pub fn sanitize_theme_wasm(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let (out, report) = sanitize_theme_native(bytes).map_err(to_js_err)?;
    let obj = js_sys::Object::new();
    let sanitized = js_sys::Uint8Array::from(out.as_slice());
    js_sys::Reflect::set(&obj, &JsValue::from_str("sanitized"), &sanitized.into())
        .map_err(|_| JsValue::from_str("set sanitized"))?;
    let report_js = serde_wasm_bindgen::to_value(&report).map_err(to_js_err)?;
    js_sys::Reflect::set(&obj, &JsValue::from_str("report"), &report_js)
        .map_err(|_| JsValue::from_str("set report"))?;
    Ok(obj.into())
}

#[wasm_bindgen(js_name = "sanitizeBundle")]
pub fn sanitize_bundle_wasm(manifest_js: JsValue, files_js: JsValue) -> Result<JsValue, JsValue> {
    let manifest: Manifest = serde_wasm_bindgen::from_value(manifest_js).map_err(to_js_err)?;
    let files = files_from_js(&files_js)?;
    let (out, report) = sanitize_bundle_native(&manifest, files).map_err(to_js_err)?;
    let ret = js_sys::Object::new();
    js_sys::Reflect::set(&ret, &JsValue::from_str("sanitized"), &files_to_js(&out)?)
        .map_err(|_| JsValue::from_str("set sanitized"))?;
    let report_js = serde_wasm_bindgen::to_value(&report).map_err(to_js_err)?;
    js_sys::Reflect::set(&ret, &JsValue::from_str("report"), &report_js)
        .map_err(|_| JsValue::from_str("set report"))?;
    Ok(ret.into())
}

/// Executable-magic deny-list surface (invariant #19c). Returns `{ ok: true }`
/// if the bytes do not start with any denied prefix, else `{ ok: false, prefixHex }`.
#[wasm_bindgen(js_name = "rejectExecutableMagic")]
pub fn reject_executable_magic_wasm(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let obj = js_sys::Object::new();
    match reject_executable_magic(bytes) {
        Ok(()) => {
            js_sys::Reflect::set(&obj, &JsValue::from_str("ok"), &JsValue::from_bool(true))
                .map_err(|_| JsValue::from_str("set ok"))?;
        }
        Err(sig) => {
            js_sys::Reflect::set(&obj, &JsValue::from_str("ok"), &JsValue::from_bool(false))
                .map_err(|_| JsValue::from_str("set ok"))?;
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("prefixHex"),
                &JsValue::from_str(&hex::encode(sig)),
            )
            .map_err(|_| JsValue::from_str("set prefixHex"))?;
        }
    }
    Ok(obj.into())
}
