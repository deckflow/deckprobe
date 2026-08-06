// Turn the archives cargo-dist already builds into npm platform packages.
//
// The binaries are not rebuilt here. Release CI downloads the dist artifacts,
// points this script at them, and publishes the result, so the npm CLI and the
// shell installers hand out byte-identical binaries.
//
//   node scripts/build-platform-packages.mjs --artifacts target/distrib --out npm
//   node scripts/build-platform-packages.mjs --binary ../../target/release/deckprobe \
//     --target aarch64-apple-darwin --out npm      # single host target, for local testing

import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { binaryName, packageName, platforms } from "../bin/platforms.js";

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const repositoryRoot = resolve(packageDirectory, "../..");
const manifest = JSON.parse(readFileSync(join(packageDirectory, "package.json"), "utf8"));

function parseArguments(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index]?.replace(/^--/, "");
    if (!key || argv[index + 1] === undefined) {
      throw new Error(`malformed argument near ${argv[index]}`);
    }
    options[key] = argv[index + 1];
  }
  return options;
}

const options = parseArguments(process.argv.slice(2));
const outputRoot = resolve(packageDirectory, options.out ?? "npm");

/** Archive names have varied across cargo-dist versions; match on the target triple. */
function findArchive(directory, platform) {
  const extension = `.${platform.archive}`;
  const candidates = readdirSync(directory).filter(
    (entry) => entry.includes(platform.target) && entry.endsWith(extension),
  );
  if (candidates.length > 1) {
    throw new Error(`ambiguous archives for ${platform.target}: ${candidates.join(", ")}`);
  }
  return candidates[0] ? join(directory, candidates[0]) : undefined;
}

/** cargo-dist has placed the binary at the archive root and in a subdirectory. */
function findBinary(directory, name) {
  for (const entry of readdirSync(directory)) {
    const candidate = join(directory, entry);
    if (entry === name && statSync(candidate).isFile()) return candidate;
    if (statSync(candidate).isDirectory()) {
      const nested = findBinary(candidate, name);
      if (nested) return nested;
    }
  }
  return undefined;
}

function extractBinary(archive, platform) {
  const scratch = mkdtempSync(join(tmpdir(), "deckprobe-dist-"));
  try {
    if (platform.archive === "zip") {
      execFileSync("unzip", ["-q", archive, "-d", scratch], { stdio: "pipe" });
    } else {
      execFileSync("tar", ["-xzf", archive, "-C", scratch], { stdio: "pipe" });
    }
    const binary = findBinary(scratch, binaryName(platform));
    if (!binary) {
      throw new Error(`archive ${archive} does not contain ${binaryName(platform)}`);
    }
    // Copy out before the scratch directory is removed.
    const staged = join(scratch, "..", `deckprobe-${platform.suffix}-${process.pid}`);
    copyFileSync(binary, staged);
    return staged;
  } finally {
    rmSync(scratch, { force: true, recursive: true });
  }
}

function writePackage(platform, binarySource) {
  const directory = join(outputRoot, platform.suffix);
  rmSync(directory, { force: true, recursive: true });
  mkdirSync(join(directory, "bin"), { recursive: true });

  const target = join(directory, "bin", binaryName(platform));
  copyFileSync(binarySource, target);
  chmodSync(target, 0o755);

  // No "bin" field: the launcher in the main package owns the deckprobe
  // command, and a second declaration would collide on install. No "exports"
  // either, so the launcher can resolve this package's package.json.
  const platformManifest = {
    name: packageName(platform),
    version: manifest.version,
    description: `Prebuilt DeckProbe CLI binary for ${platform.os} ${platform.cpu}${
      platform.libc ? ` (${platform.libc})` : ""
    }`,
    license: manifest.license,
    repository: manifest.repository,
    os: [platform.os],
    cpu: [platform.cpu],
    ...(platform.libc ? { libc: [platform.libc] } : {}),
    files: ["bin"],
    engines: manifest.engines,
    publishConfig: manifest.publishConfig,
    // Yarn Berry would otherwise store the binary inside a zip, where it
    // cannot be executed.
    preferUnplugged: true,
  };
  writeFileSync(
    join(directory, "package.json"),
    `${JSON.stringify(platformManifest, undefined, 2)}\n`,
  );
  copyFileSync(join(repositoryRoot, "LICENSE"), join(directory, "LICENSE"));
  writeFileSync(
    join(directory, "README.md"),
    `# ${packageName(platform)}\n\n` +
      `Prebuilt \`deckprobe\` binary for ${platform.os} ${platform.cpu}` +
      `${platform.libc ? ` (${platform.libc})` : ""}, target \`${platform.target}\`.\n\n` +
      "This package is an implementation detail of [`@deckflow/deckprobe`]" +
      "(https://www.npmjs.com/package/@deckflow/deckprobe), which installs it\n" +
      "automatically for the current platform. Install that package instead.\n",
  );
  return directory;
}

const built = [];

if (options.binary) {
  if (!options.target) throw new Error("--binary requires --target");
  const platform = platforms.find((entry) => entry.target === options.target);
  if (!platform) throw new Error(`unknown target ${options.target}`);
  built.push([platform, writePackage(platform, resolve(options.binary))]);
} else {
  const artifacts = resolve(options.artifacts ?? join(repositoryRoot, "target/distrib"));
  if (!existsSync(artifacts)) throw new Error(`artifact directory not found: ${artifacts}`);
  const missing = [];
  for (const platform of platforms) {
    const archive = findArchive(artifacts, platform);
    if (!archive) {
      missing.push(platform.target);
      continue;
    }
    const staged = extractBinary(archive, platform);
    try {
      built.push([platform, writePackage(platform, staged)]);
    } finally {
      rmSync(staged, { force: true });
    }
  }
  if (missing.length) {
    // A partial matrix would publish a version some platforms can never
    // resolve, so refuse rather than ship a half-built release.
    throw new Error(`no archive found for: ${missing.join(", ")}`);
  }
}

for (const [platform, directory] of built) {
  console.log(`${packageName(platform)}@${manifest.version} -> ${directory}`);
}
