/**
 * Ambient declarations for the wasm-bindgen-generated JS shims. Without
 * `allowJs`, TypeScript can't infer the surface of the `.js` files at
 * `src/wasm/`, so we declare the exports we actually use. Exhaustive-enough
 * for the loader + consumers; extend as new wasm entrypoints get wired.
 *
 * The `#[wasm_bindgen]` source of truth lives in `crates/omni-{bundle,
 * identity,sanitize}/src/wasm.rs`. Keep these signatures in lockstep.
 */

declare module "*.wasm" {
  const module: WebAssembly.Module;
  export default module;
}

declare module "*/omni_bundle.js" {
  export function initSync(opts: { module: WebAssembly.Module }): unknown;
  export default function init(
    moduleOrPath?: WebAssembly.Module | URL | string,
  ): Promise<unknown>;
  export function canonicalHash(manifest: unknown): Uint8Array;
  export function pack(manifest: unknown, files: unknown, limits?: unknown): Uint8Array;
  export function unpack(bytes: Uint8Array, limits?: unknown): WasmUnpackHandle;
  export function unpackManifest(bytes: Uint8Array, limits?: unknown): unknown;
  export class WasmUnpackHandle {
    manifest(): unknown;
    next(): { path: string; bytes: Uint8Array } | null;
    free(): void;
  }
}

declare module "*/omni_identity.js" {
  export function initSync(opts: { module: WebAssembly.Module }): unknown;
  export default function init(
    moduleOrPath?: WebAssembly.Module | URL | string,
  ): Promise<unknown>;
  export function canonicalHash(manifest: unknown): Uint8Array;
  export function pack(manifest: unknown, files: unknown, limits?: unknown): Uint8Array;
  export function unpack(bytes: Uint8Array, limits?: unknown): unknown;
  export function unpackManifest(bytes: Uint8Array, limits?: unknown): unknown;
  export function signJws(claims: unknown, privKey: Uint8Array): string;
  export function verifyJws(token: string, pubkey: Uint8Array): unknown;
  export function packSignedBundle(
    manifest: unknown,
    files: unknown,
    privKey: Uint8Array,
    limits?: unknown,
  ): Uint8Array;
  export function unpackSignedBundle(
    bytes: Uint8Array,
    limits?: unknown,
  ): WasmSignedBundleHandle;
  export class WasmSignedBundleHandle {
    manifest(): unknown;
    authorPubkey(): Uint8Array;
    nextFile(): { path: string; bytes: Uint8Array } | null;
    free(): void;
  }
}

declare module "*/omni_sanitize.js" {
  export function initSync(opts: { module: WebAssembly.Module }): unknown;
  export default function init(
    moduleOrPath?: WebAssembly.Module | URL | string,
  ): Promise<unknown>;
  export function canonicalHash(manifest: unknown): Uint8Array;
  export function pack(manifest: unknown, files: unknown, limits?: unknown): Uint8Array;
  export function unpack(bytes: Uint8Array, limits?: unknown): unknown;
  export function unpackManifest(bytes: Uint8Array, limits?: unknown): unknown;
  export function sanitizeTheme(bytes: Uint8Array): {
    sanitized: Uint8Array;
    report: unknown;
  };
  export function sanitizeBundle(
    manifest: unknown,
    files: unknown,
  ): { sanitized: Record<string, Uint8Array>; report: unknown };
  export function rejectExecutableMagic(bytes: Uint8Array): {
    ok: boolean;
    prefixHex?: string;
  };
}
