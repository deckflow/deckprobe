// Node entry point, selected by the "node" export condition.
//
// It re-exports the shared API unchanged and fixes the one thing that cannot
// work under Node: wasm-bindgen's default loader fetches a relative URL, and
// Node's fetch does not implement `file:`. Reading the binary from disk here
// keeps lazy initialization working, so `probe()` needs no special treatment.

import { readFileSync } from "node:fs";
import { basename } from "node:path";
import { fileURLToPath } from "node:url";

import { probe } from "./index.js";
import { configureRuntime } from "./runtime.js";

import type { ProbeCallOptions, ProbeResult } from "./types.js";

export * from "./index.js";

const wasmPath = fileURLToPath(new URL("../wasm/deckprobe_wasm_bg.wasm", import.meta.url));

configureRuntime({
  wasmSource: () => readFileSync(wasmPath),
  sourceKind: "node_bytes",
});

/** Absolute path to the WebAssembly binary this package will load. */
export const deckProbeWasmPath: string = wasmPath;

/**
 * Probe a file on disk. The report names the file and records
 * `source_kind: "local_file"`, matching what the native CLI writes for the
 * same input.
 *
 * The whole file is read into memory, because the WebAssembly engine takes
 * bytes rather than a file handle. The native CLI reads lazily instead, so
 * prefer it for inputs large enough that holding them in memory matters.
 */
export async function probeFile(
  path: string,
  options: ProbeCallOptions = {},
): Promise<ProbeResult> {
  const bytes = readFileSync(path);
  return probe(new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength), {
    ...options,
    name: options.name ?? basename(path),
    sourceKind: options.sourceKind ?? "local_file",
  });
}
