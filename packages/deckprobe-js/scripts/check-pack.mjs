import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const repositoryRoot = resolve(packageDirectory, "../..");
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
  "LICENSE",
  "NOTICE",
  "bin/deckprobe.js",
  "dist/index.js",
  "dist/index.node.js",
  "dist/node-runner.js",
  "dist/node-worker.js",
  "dist/node-options.js",
  "bin/platforms.js",
  "dist/worker.js",
  "wasm/deckprobe_wasm.js",
  "wasm/deckprobe_wasm_bg.wasm",
];

for (const path of required) {
  assert.ok(files.has(path), `packed tarball is missing ${path}`);
  assert.ok(files.get(path) > 0, `packed tarball contains an empty ${path}`);
}

for (const path of files.keys()) {
  assert.ok(!path.endsWith(".map"), `packed tarball contains source map ${path}`);
  assert.ok(!path.startsWith("src/"), `packed tarball contains TypeScript source ${path}`);
}

// Keep the standalone package's notices identical to the repository originals.
for (const path of ["LICENSE", "NOTICE"]) {
  assert.equal(
    readFileSync(resolve(packageDirectory, path), "utf8"),
    readFileSync(resolve(repositoryRoot, path), "utf8"),
    `package ${path} must match the repository root`,
  );
}

console.log(
  `Packed artifact is complete: ${packed.filename} ` +
    `(${packed.entryCount} files, ${required.length} required artifacts verified, ` +
    `no source maps or TypeScript sources)`,
);
