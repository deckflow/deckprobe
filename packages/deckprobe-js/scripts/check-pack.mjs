import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const output = execFileSync(
  "npm",
  ["pack", "--dry-run", "--json", "--ignore-scripts"],
  { cwd: packageDirectory, encoding: "utf8" },
);
const [packed] = JSON.parse(output);
assert.ok(packed, "npm pack did not describe an output tarball");

const files = new Map(
  packed.files.map((entry) => [entry.path.replaceAll("\\", "/"), entry.size]),
);
const required = [
  "bin/deckprobe.js",
  "dist/index.js",
  "dist/index.node.js",
  "dist/worker.js",
  "wasm/deckprobe_wasm.js",
  "wasm/deckprobe_wasm_bg.wasm",
];

for (const path of required) {
  assert.ok(files.has(path), `packed tarball is missing ${path}`);
  assert.ok(files.get(path) > 0, `packed tarball contains an empty ${path}`);
}

console.log(
  `Packed artifact is complete: ${packed.filename} ` +
    `(${packed.entryCount} files, ${required.length} required artifacts verified)`,
);
