import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { packageName, platforms } from "../bin/platforms.js";

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const repositoryRoot = resolve(packageDirectory, "../..");
const packageJson = JSON.parse(
  readFileSync(resolve(packageDirectory, "package.json"), "utf8"),
);
const cargoManifest = readFileSync(resolve(repositoryRoot, "Cargo.toml"), "utf8");
const workspaceVersion = cargoManifest.match(
  /\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
)?.[1];

assert.ok(workspaceVersion, "Cargo workspace version is missing");
assert.equal(
  packageJson.version,
  workspaceVersion,
  "npm and Cargo workspace versions must match",
);

// The CLI binary lives in per-platform optional dependencies. A version skew
// here resolves an old binary against a new wrapper, or nothing at all, so pin
// them to this package's exact version and check the matrix is complete.
const expected = platforms.map((platform) => packageName(platform)).sort();
const declared = Object.keys(packageJson.optionalDependencies ?? {}).sort();
assert.deepEqual(
  declared,
  expected,
  "optionalDependencies must list exactly the platform packages in bin/platforms.js",
);
for (const [name, range] of Object.entries(packageJson.optionalDependencies ?? {})) {
  assert.equal(
    range,
    packageJson.version,
    `${name} must be pinned to ${packageJson.version}, found ${range}`,
  );
}

console.log(
  `DeckProbe package versions agree: ${packageJson.version} ` +
    `(${declared.length} platform packages pinned)`,
);
