import type { InitInput } from "../wasm/deckprobe_wasm.js";

/**
 * Host-specific defaults, set once by whichever entry point the runtime
 * resolved. The browser build leaves both alone; the Node build installs a
 * filesystem loader, because wasm-bindgen's default fetch of a relative URL
 * cannot read `file:` in Node.
 *
 * This lives apart from index.ts so both entry points share one instance of
 * the state. Importing it from either side must not pull in `node:` modules.
 */

let wasmSource: (() => InitInput) | undefined;
let sourceKind = "browser_bytes";

export function configureRuntime(defaults: {
  wasmSource?: () => InitInput;
  sourceKind?: string;
}): void {
  if (defaults.wasmSource) wasmSource = defaults.wasmSource;
  if (defaults.sourceKind) sourceKind = defaults.sourceKind;
}

/** Undefined keeps wasm-bindgen's own relative-URL resolution. */
export function defaultWasmSource(): InitInput | undefined {
  return wasmSource?.();
}

export function defaultSourceKind(): string {
  return sourceKind;
}
