import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

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

console.log(`DeckProbe package versions agree: ${packageJson.version}`);
