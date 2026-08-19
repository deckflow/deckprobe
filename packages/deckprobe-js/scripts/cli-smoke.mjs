// Parity guard for the npm-installed CLI.
//
// The binary is the same one the native installers ship, so fidelity of the
// probe results is guaranteed by construction. What is NOT guaranteed is the
// launcher: argv passthrough, stdin passthrough, stdout being handed over
// untouched, and exit codes surviving the extra process. Every assertion below
// compares the launcher against the binary it wraps, byte for byte.

import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { binaryName, currentPlatform, packageName, platforms } from "../bin/platforms.js";

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const repositoryRoot = resolve(packageDirectory, "../..");
const nativeBinary = join(repositoryRoot, "target/release/deckprobe");
const fixtures = join(repositoryRoot, "tests/fixtures/local");

const expectedVersion = JSON.parse(
  readFileSync(join(packageDirectory, "package.json"), "utf8"),
).version;

const platform = currentPlatform();
assert.ok(platform, `no platform package defined for ${process.platform}-${process.arch}`);
assert.ok(
  existsSync(nativeBinary),
  `missing ${nativeBinary}. Run: cargo build --locked --release -p deckprobe`,
);
assert.ok(
  existsSync(fixtures),
  `missing ${fixtures}. These fixtures are private and are not published to the ` +
    `open-source tree, so this suite cannot run there. Use "npm run test:public", ` +
    `which omits the fixture-dependent suites.`,
);

const workspace = mkdtempSync(join(tmpdir(), "deckprobe-cli-"));
const modules = join(workspace, "node_modules", "@deckflow");

function installPackages() {
  mkdirSync(modules, { recursive: true });
  // Build the platform package from the same binary used as the reference, so
  // a mismatch can only come from the launcher.
  execFileSync(
    process.execPath,
    [
      join(packageDirectory, "scripts/build-platform-packages.mjs"),
      "--binary",
      nativeBinary,
      "--target",
      platform.target,
      "--out",
      join(workspace, "platform"),
    ],
    { stdio: "pipe" },
  );
  execFileSync("cp", ["-R", join(workspace, "platform", platform.suffix), join(modules, `deckprobe-${platform.suffix}`)]);

  // Unpack the real tarball so the launcher runs from the published layout,
  // including whatever "files" actually lets through.
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

  const launcher = join(modules, "deckprobe", "bin", "deckprobe.js");
  assert.ok(existsSync(launcher), "the packed tarball does not contain bin/deckprobe.js");
  chmodSync(launcher, 0o755);
  return launcher;
}

/**
 * Release CI only ever calls the packaging script in --artifacts mode, reading
 * the archives cargo-dist produced. Exercise that path against synthesized
 * archives shaped like the real ones: the host binary stands in for every
 * target, since what is under test is discovery, extraction, and metadata.
 */
function verifyArtifactPackaging() {
  const distrib = join(workspace, "distrib");
  const staging = join(workspace, "staging");
  mkdirSync(distrib, { recursive: true });
  for (const entry of platforms) {
    rmSync(staging, { force: true, recursive: true });
    const name = binaryName(entry);
    if (entry.archive === "zip") {
      mkdirSync(staging, { recursive: true });
      execFileSync("cp", [nativeBinary, join(staging, name)]);
      execFileSync("zip", ["-q", join(distrib, `deckprobe-1.0.0-${entry.target}.zip`), name], {
        cwd: staging,
      });
    } else {
      // cargo-dist has shipped the binary inside a versioned directory.
      const inner = join(staging, `deckprobe-1.0.0-${entry.target}`);
      mkdirSync(inner, { recursive: true });
      execFileSync("cp", [nativeBinary, join(inner, name)]);
      execFileSync("tar", [
        "-czf",
        join(distrib, `deckprobe-1.0.0-${entry.target}.tar.gz`),
        "-C",
        staging,
        `deckprobe-1.0.0-${entry.target}`,
      ]);
    }
  }

  const out = join(workspace, "platform-packages");
  execFileSync(
    process.execPath,
    [
      join(packageDirectory, "scripts/build-platform-packages.mjs"),
      "--artifacts",
      distrib,
      "--out",
      out,
    ],
    { stdio: "pipe" },
  );

  for (const entry of platforms) {
    const directory = join(out, entry.suffix);
    const built = JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));
    assert.equal(built.name, packageName(entry));
    assert.equal(built.version, expectedVersion);
    assert.deepEqual(built.os, [entry.os], `${entry.suffix} os`);
    assert.deepEqual(built.cpu, [entry.cpu], `${entry.suffix} cpu`);
    assert.deepEqual(
      built.libc,
      entry.libc ? [entry.libc] : undefined,
      `${entry.suffix} libc must distinguish glibc from musl`,
    );
    // A bin field here would collide with the launcher the main package links.
    assert.equal(built.bin, undefined, `${entry.suffix} must not declare bin`);
    // No exports field, so the launcher can resolve package.json.
    assert.equal(built.exports, undefined, `${entry.suffix} must not declare exports`);
    const binary = join(directory, "bin", binaryName(entry));
    assert.ok(existsSync(binary), `${entry.suffix} is missing ${binaryName(entry)}`);
    assert.equal(statSync(binary).mode & 0o111, 0o111, `${entry.suffix} binary is not executable`);
  }

  // A partial matrix would publish a version some platforms can never resolve.
  const partial = join(workspace, "partial");
  mkdirSync(partial, { recursive: true });
  execFileSync("cp", [
    join(distrib, `deckprobe-1.0.0-${platforms[0].target}.${platforms[0].archive}`),
    partial,
  ]);
  const incomplete = spawnSync(process.execPath, [
    join(packageDirectory, "scripts/build-platform-packages.mjs"),
    "--artifacts",
    partial,
    "--out",
    join(workspace, "partial-out"),
  ]);
  assert.notEqual(incomplete.status, 0, "an incomplete artifact set must not build packages");
  assert.match(incomplete.stderr.toString(), /no archive found for/);

  return platforms.length;
}

function runNative(args, input) {
  return spawnSync(nativeBinary, args, { input, maxBuffer: 64 * 1024 * 1024 });
}

function runLauncher(launcher, args, input) {
  return spawnSync(process.execPath, [launcher, ...args], {
    input,
    maxBuffer: 64 * 1024 * 1024,
  });
}

function comparable(result) {
  return {
    status: result.status,
    stdout: result.stdout.toString("base64"),
    stderr: result.stderr.toString("base64"),
  };
}

const failures = [];

function parity(launcher, label, args, input) {
  const native = runNative(args, input);
  const wrapped = runLauncher(launcher, args, input);
  try {
    assert.deepEqual(comparable(wrapped), comparable(native), label);
    return true;
  } catch {
    failures.push(
      `${label}\n  argv: ${JSON.stringify(args)}\n` +
        `  exit: native=${native.status} launcher=${wrapped.status}\n` +
        `  stdout bytes: native=${native.stdout.length} launcher=${wrapped.stdout.length}\n` +
        `  stderr(native): ${native.stderr.toString().slice(0, 300)}\n` +
        `  stderr(launcher): ${wrapped.stderr.toString().slice(0, 300)}`,
    );
    return false;
  }
}

try {
  const packaged = verifyArtifactPackaging();
  const launcher = installPackages();
  let checks = 0;

  // The stated requirement: `deckprobe -h` must work, and match the native CLI.
  const helpArgs = [
    ["--help"],
    ["-h"],
    ["--version"],
    ["-V"],
    ["formats", "--help"],
    ["targets", "--help"],
    ["generate", "--help"],
    ["schema", "--help"],
    ["completion", "--help"],
    ["install", "--help"],
    ["help"],
  ];
  for (const args of helpArgs) {
    if (parity(launcher, `help parity ${args.join(" ")}`, args)) checks += 1;
  }
  // Guard against a launcher that "passes" by producing empty output.
  const help = runLauncher(launcher, ["--help"]).stdout.toString();
  assert.match(help, /Usage: deckprobe/, "--help did not render the usage block");
  assert.match(help, /Commands:/, "--help did not render the subcommand section");

  // Discovery and probe output across every fixture format.
  const documents = readdirSync(fixtures).filter((entry) => !entry.startsWith("."));
  assert.ok(documents.length > 0, "no fixtures found");
  for (const document of documents) {
    const path = join(fixtures, document);
    for (const args of [
      ["-t", "@summary", path],
      ["-t", "@summary", "--view", "values", path],
      ["--pretty", "-l", "h", "-t", "@header", path],
      ["-P", "-t", "@default", path],
    ]) {
      if (parity(launcher, `probe parity ${document}`, args)) checks += 1;
    }
  }
  for (const args of [["formats"], ["schema"], ["targets", "pdf"], ["targets", "pptx", "--pretty"]]) {
    if (parity(launcher, `discovery parity ${args.join(" ")}`, args)) checks += 1;
  }

  // The agent skill is embedded in the binary, so `npx @deckflow/deckprobe
  // install --skills` works without the package shipping a second copy. Compare
  // in --dry-run so the check cannot touch the developer's real skill
  // directories, and with an explicit --dir so the result does not depend on
  // which agents happen to be installed on the runner.
  const skillDirectory = join(workspace, "skills");
  for (const args of [
    ["install", "--skills", "--dry-run", "--dir", skillDirectory],
    ["install", "--dry-run", "--pretty", "--dir", skillDirectory],
  ]) {
    if (parity(launcher, `install parity ${args.join(" ")}`, args)) checks += 1;
  }
  const receipt = JSON.parse(
    runLauncher(launcher, ["install", "--skills", "--dry-run", "--dir", skillDirectory]).stdout,
  );
  assert.equal(receipt.status, "ok", "install did not report success");
  assert.ok(
    receipt.install.targets[0].files.some((file) => file.path === "SKILL.md"),
    "install did not plan a SKILL.md; the skill may not be embedded in this binary",
  );

  // Exit codes must survive the extra process, including the CLI syntax path
  // that the launcher never sees the binary produce.
  for (const args of [
    ["--not-a-flag"],
    ["/nonexistent/file.pdf"],
    ["-t", "not_a_target", join(fixtures, "pdf-minimal.pdf")],
    ["-s", "-c", "x", "-t", "@all", join(fixtures, "powerpoint-basic.pptx")],
    ["-n", "x.pdf"],
  ]) {
    if (parity(launcher, `exit-code parity ${args.join(" ")}`, args)) checks += 1;
  }

  // stdin passthrough: bytes on stdin, and the JSONL record stream.
  const pdf = execFileSync("cat", [join(fixtures, "pdf-metadata.pdf")], {
    maxBuffer: 64 * 1024 * 1024,
  });
  if (parity(launcher, "stdin document", ["-n", "pdf-metadata.pdf", "-", "-t", "@summary"], pdf)) {
    checks += 1;
  }
  const records = documents.map((name) => JSON.stringify({ path: join(fixtures, name) })).join("\n");
  if (parity(launcher, "jsonl stream", ["--jsonl", "-t", "@summary"], `${records}\n`)) {
    checks += 1;
  }
  // One report per input line, in order.
  const jsonl = runLauncher(launcher, ["--jsonl", "-t", "@summary"], `${records}\n`)
    .stdout.toString()
    .trim()
    .split("\n");
  assert.equal(jsonl.length, documents.length, "JSONL did not emit one report per record");
  for (const line of jsonl) {
    assert.equal(JSON.parse(line).schema_version, 2, "JSONL emitted a non-v2 report");
  }

  if (!failures.length) {
    console.log(
      `CLI parity passed: ${checks} argv comparisons against ${nativeBinary} ` +
        `via ${packageName(platform)}; ${packaged} platform packages built from archives`,
    );
  }
} finally {
  rmSync(workspace, { force: true, recursive: true });
  // Report in the finally so a hard assertion later in the run cannot hide the
  // parity mismatches collected before it.
  if (failures.length) {
    console.error(`\n${failures.length} parity failure(s):\n\n${failures.join("\n\n")}`);
    process.exitCode = 1;
  }
}
