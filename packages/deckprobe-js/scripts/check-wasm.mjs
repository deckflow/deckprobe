import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const wasmDirectory = resolve(fileURLToPath(new URL("../wasm/", import.meta.url)));
const required = ["deckprobe_wasm.js", "deckprobe_wasm_bg.wasm"];
const missing = required.filter((name) => {
  const path = resolve(wasmDirectory, name);
  return !existsSync(path) || !statSync(path).isFile() || statSync(path).size === 0;
});

const binaryPath = resolve(wasmDirectory, "deckprobe_wasm_bg.wasm");
const binaryHeader = existsSync(binaryPath) ? readFileSync(binaryPath).subarray(0, 4) : undefined;
const validWebAssembly = binaryHeader?.every((value, index) => value === [0, 0x61, 0x73, 0x6d][index]);

if (missing.length || !validWebAssembly) {
  console.error("DeckProbe WASM artifacts are missing or invalid.");
  console.error(`Expected: ${wasmDirectory}/deckprobe_wasm.js and deckprobe_wasm_bg.wasm`);
  console.error("Run `npm run build:wasm` (or `npm run benchmark:build`) with cargo on PATH.");
  process.exit(1);
}

console.log(`WASM artifacts ready: ${wasmDirectory}`);
