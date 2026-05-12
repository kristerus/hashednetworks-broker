/**
 * Canonical signing for broker messages.
 *
 * Server-side canonicalization (Rust):
 *   serde_json::to_value(msg)                         // object with default-BTreeMap ordering
 *   obj.insert("signature".into(), Value::String("")) // zero out signature field
 *   serde_json::to_vec(&v)                            // emit JSON bytes
 *
 * `serde_json::Map` defaults to `BTreeMap<String, Value>` when the
 * `preserve_order` feature is OFF — which is the broker's configuration. So
 * the output is JSON with **alphabetically-sorted keys**, recursively.
 *
 * To produce matching bytes from TypeScript we must:
 *   1. Set `signature` to "" before stringifying.
 *   2. Stringify with alphabetically-sorted keys at every nesting level.
 *   3. Skip undefined values (matching `#[serde(skip_serializing_if = "Option::is_none")]`).
 */

import * as ed25519 from "@noble/ed25519";

// @noble/ed25519 v2 uses SubtleCrypto for SHA-512 — available natively on
// Node 20+ and every modern browser. No extra wiring needed.

/** Recursively serialize `value` with object keys alphabetically sorted. */
export function canonicalJson(value: unknown): string {
  return JSON.stringify(value, sortReplacer);
}

function sortReplacer(_key: string, value: unknown): unknown {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return value;
  }
  const src = value as Record<string, unknown>;
  const sorted: Record<string, unknown> = {};
  for (const k of Object.keys(src).sort()) {
    const v = src[k];
    if (v === undefined) continue;
    sorted[k] = v;
  }
  return sorted;
}

const encoder = new TextEncoder();

/** Sign a (yet-to-be-signed) message object with the supplied 32-byte seed. */
export async function signMessage<T extends Record<string, unknown>>(
  privateKey: Uint8Array,
  message: T
): Promise<T & { signature: string }> {
  const draft = { ...message, signature: "" };
  const canonical = encoder.encode(canonicalJson(draft));
  const sig = await ed25519.signAsync(canonical, privateKey);
  return { ...message, signature: bytesToHex(sig) } as T & { signature: string };
}

// ---------------------------------------------------------------------------
// Hex helpers — kept inline to avoid pulling another dependency.
// ---------------------------------------------------------------------------

export function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i] as number;
    out += (b < 16 ? "0" : "") + b.toString(16);
  }
  return out;
}

export function hexToBytes(hex: string): Uint8Array {
  const clean = hex.startsWith("ed25519:") ? hex.slice(8) : hex;
  if (clean.length % 2 !== 0) {
    throw new Error(`hex has odd length: ${clean.length}`);
  }
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/** Generate a new Ed25519 keypair (32-byte seed + 32-byte verifying key). */
export async function generateKeypair(): Promise<{
  privateKey: Uint8Array;
  publicKey: Uint8Array;
  publicKeyHex: string;
}> {
  const privateKey = ed25519.utils.randomPrivateKey();
  const publicKey = await ed25519.getPublicKeyAsync(privateKey);
  return { privateKey, publicKey, publicKeyHex: bytesToHex(publicKey) };
}
