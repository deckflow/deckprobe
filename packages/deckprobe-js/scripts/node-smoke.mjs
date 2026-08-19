// Contract test for the Node entry point.
//
// Runs against the packed tarball inside a real node_modules so the "node"
// export condition is exercised the way a consumer hits it. Importing the
// working-tree file directly would bypass the very resolution being tested.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const repositoryRoot = resolve(packageDirectory, "../..");
const fixtures = join(repositoryRoot, "tests/fixtures/local");
const nativeBinary = join(repositoryRoot, "target/release/deckprobe");
const expectedVersion = JSON.parse(
  readFileSync(join(packageDirectory, "package.json"), "utf8"),
).version;

const workspace = mkdtempSync(join(tmpdir(), "deckprobe-node-"));

try {
  const modules = join(workspace, "node_modules", "@deckflow");
  mkdirSync(modules, { recursive: true });
  const tarball = execFileSync("npm", ["pack", "--silent", "--pack-destination", workspace], {
    cwd: packageDirectory,
    encoding: "utf8",
  })
    .split("\n")
    .map((line) => line.trim())
    .findLast((line) => line.endsWith(".tgz"));
  assert.ok(tarball, "npm pack did not report a tarball");
  execFileSync("tar", ["-xzf", join(workspace, tarball), "-C", workspace]);
  execFileSync("mv", [join(workspace, "package"), join(modules, "deckprobe")]);

  const entry = join(workspace, "consumer.mjs");
  const require = createRequire(join(workspace, "index.js"));

  // The "node" condition must win here, and the browser build must stay
  // reachable so a bundler still gets the fetch-based entry.
  const resolved = require.resolve("@deckflow/deckprobe");
  assert.match(
    resolved,
    /index\.node\.js$/,
    `Node resolved ${resolved} instead of the node entry point`,
  );
  assert.ok(
    existsSync(join(modules, "deckprobe", "dist", "index.js")),
    "the browser entry point is missing from the tarball",
  );

  assert.ok(
    existsSync(fixtures),
    `missing ${fixtures}. These fixtures are private and are not published to the ` +
      `open-source tree, so this suite cannot run there. Use "npm run test:public", ` +
      `which omits the fixture-dependent suites.`,
  );

  const documents = readdirSync(fixtures).filter((name) => !name.startsWith("."));
  assert.ok(documents.length > 0, "no fixtures found");

  const script = `
import { readFileSync } from "node:fs";
import {
  deckProbeWasmPath,
  formats,
  initDeckProbe,
  probe,
  probeFile,
  schema,
  targets,
  version,
} from "@deckflow/deckprobe";

const documents = ${JSON.stringify(documents.map((name) => join(fixtures, name)))};
const pdf = ${JSON.stringify(join(fixtures, "pdf-metadata.pdf"))};

// No argument: this is what failed with "fetch failed" before the node entry
// point existed.
await initDeckProbe();

const reports = {};
for (const path of documents) {
  reports[path] = await probeFile(path, { targets: ["@summary"], level: "metadata" });
}

const bytes = readFileSync(pdf);
const byteOptions = { name: "pdf-metadata.pdf", targets: ["@header"], level: "header" };
const asBuffer = await probe(bytes, byteOptions);
const asUint8 = await probe(new Uint8Array(bytes), byteOptions);
const asArrayBuffer = await probe(
  bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
  byteOptions,
);

let missingName;
try {
  await probe(new Uint8Array([1, 2, 3]), { targets: ["@header"] });
} catch (error) {
  missingName = error.constructor.name;
}

process.stdout.write(JSON.stringify({
  version: await version(),
  wasmPath: deckProbeWasmPath,
  reports,
  asBuffer,
  asUint8,
  asArrayBuffer,
  missingName,
  formatCount: (await formats()).formats.length,
  targetsStatus: (await targets("pdf")).status,
  schemaVersion: (await schema()).$schema !== undefined,
}));
`;
  writeFileSync(entry, script);

  const outcome = JSON.parse(
    execFileSync(process.execPath, [entry], {
      cwd: workspace,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    }),
  );

  assert.equal(outcome.version, expectedVersion, "the Node entry reported the wrong version");
  assert.ok(existsSync(outcome.wasmPath), "deckProbeWasmPath does not point at a real file");
  assert.equal(outcome.missingName, "TypeError", "bytes without a name must throw TypeError");
  assert.ok(outcome.formatCount > 0, "formats() returned nothing");
  assert.equal(outcome.targetsStatus, "ok", "targets() failed");
  assert.equal(outcome.schemaVersion, true, "schema() did not return a JSON Schema");

  // Every byte input shape must produce the same report.
  assert.deepEqual(outcome.asUint8, outcome.asBuffer, "Uint8Array and Buffer disagree");
  assert.deepEqual(outcome.asArrayBuffer, outcome.asBuffer, "ArrayBuffer and Buffer disagree");
  assert.equal(outcome.asBuffer.input.source_kind, "node_bytes");

  // probeFile must match what the native CLI writes for the same input. This is
  // the strongest available statement that the Node API and the CLI agree.
  assert.ok(
    existsSync(nativeBinary),
    `missing ${nativeBinary}. Run: cargo build --locked --release -p deckprobe`,
  );
  let compared = 0;
  for (const [path, report] of Object.entries(outcome.reports)) {
    assert.equal(report.input.source_kind, "local_file", `${path} did not report local_file`);
    const native = JSON.parse(
      execFileSync(nativeBinary, ["-t", "@summary", "-l", "m", path], {
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
      }),
    );
    assert.deepEqual(report, native, `probeFile disagrees with the native CLI for ${path}`);
    compared += 1;
  }

  console.log(
    `Node smoke passed: ${compared} reports identical to the native CLI, ` +
      `3 byte-input shapes, discovery API available`,
  );
} finally {
  rmSync(workspace, { force: true, recursive: true });
}
