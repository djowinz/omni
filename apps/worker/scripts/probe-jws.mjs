// Probe: does @tsndr/cloudflare-worker-jwt handle EdDSA with detached payloads?
// Contract target: compact = protected-b64 || "." || "" || "." || sig-b64
//                  signing input = protected-b64 || "." || payload-b64
// where payload-b64 is the b64url(sha256(body)) reconstructed by the verifier.
//
// Probe 1: library sign + library verify, attached payload, EdDSA.
// Probe 2: produce a detached compact (empty middle segment), try to verify via
//          (a) the library, and (b) Web Crypto + manual signing-input reconstruction.

import jwt from '@tsndr/cloudflare-worker-jwt';
import * as ed from '@noble/ed25519';

const b64u = (buf) =>
  Buffer.from(buf).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

const enc = new TextEncoder();
const priv = ed.utils.randomSecretKey();
const pub = await ed.getPublicKeyAsync(priv);
const kid = b64u(pub);

// --- Probe 1: library attached sign/verify, EdDSA ---
console.log('=== Probe 1: library attached sign/verify (EdDSA) ===');
let p1_ok = false;
try {
  const token = await jwt.sign(
    { sub: 'probe', iat: Math.floor(Date.now() / 1000) },
    priv, // noble 3.x secret key (32 bytes)
    { algorithm: 'EdDSA', header: { typ: 'Omni-HTTP-JWS', kid } },
  );
  console.log('  signed token:', token.slice(0, 60) + '…');
  p1_ok = await jwt.verify(token, pub, { algorithm: 'EdDSA' });
  console.log('  library verify (attached):', p1_ok);
} catch (e) {
  console.log('  library attached path FAILED:', e?.message || e);
}

// --- Probe 2: detached payload path ---
console.log('\n=== Probe 2: detached payload (reconstructed signing input) ===');
const body = enc.encode('{"hello":"world"}');
const bodyHash = await crypto.subtle.digest('SHA-256', body);
const payload_b64 = b64u(bodyHash); // b64url(sha256(body)) per Omni contract
const header = { alg: 'EdDSA', typ: 'Omni-HTTP-JWS', kid };
const protected_b64 = b64u(enc.encode(JSON.stringify(header)));
const signingInput = enc.encode(protected_b64 + '.' + payload_b64);
const sig = await ed.signAsync(signingInput, priv);
const sig_b64 = b64u(sig);
const detachedCompact = protected_b64 + '..' + sig_b64; // empty middle segment
console.log('  detached compact:', detachedCompact.slice(0, 60) + '…');

// 2a: ask the library to verify the detached form directly (will likely fail:
//     lib will decode the empty payload segment as the actual payload).
let p2a_ok = false;
try {
  p2a_ok = await jwt.verify(detachedCompact, pub, { algorithm: 'EdDSA' });
  console.log('  library verify(detached compact):', p2a_ok);
} catch (e) {
  console.log('  library verify(detached compact) threw:', e?.message || e);
}

// 2b: reconstruct signing input manually and ask the library to verify the
//     *reconstructed* attached form. The library signs/verifies over
//     header.payload — so if we hand it header.payload_b64.sig, does it verify?
let p2b_ok = false;
try {
  const reconstructedCompact = protected_b64 + '.' + payload_b64 + '.' + sig_b64;
  p2b_ok = await jwt.verify(reconstructedCompact, pub, { algorithm: 'EdDSA' });
  console.log('  library verify(reconstructed compact header.payload.sig):', p2b_ok);
} catch (e) {
  console.log('  library verify(reconstructed) threw:', e?.message || e);
}

// 2c: Web Crypto ground truth — verify the signature directly against the
//     reconstructed signing-input bytes. This is the FALLBACK_WEBCRYPTO path.
let p2c_ok = false;
try {
  const key = await crypto.subtle.importKey('raw', pub, { name: 'Ed25519' }, false, ['verify']);
  p2c_ok = await crypto.subtle.verify('Ed25519', key, sig, signingInput);
  console.log('  webcrypto verify(signingInput):', p2c_ok);
} catch (e) {
  console.log('  webcrypto verify threw:', e?.message || e);
}

console.log('\n=== Summary ===');
console.log(
  JSON.stringify(
    {
      library: '@tsndr/cloudflare-worker-jwt@3.2.1',
      noble: '@noble/ed25519@3.1.0',
      probe1_attached_lib: p1_ok,
      probe2a_lib_detached_compact: p2a_ok,
      probe2b_lib_reconstructed: p2b_ok,
      probe2c_webcrypto_signingInput: p2c_ok,
    },
    null,
    2,
  ),
);
