#!/usr/bin/env node
// Launcher for the native DeckProbe CLI.
//
// The binary itself lives in a per-platform optional dependency, so this file
// only resolves the right one and hands the process over. Everything the user
// sees -- argument parsing, --help, exit codes -- comes from the same binary
// the native installers ship, which is why there is no argument handling here.

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

import { binaryName, currentPlatform, packageName, platforms } from "./platforms.js";

const require = createRequire(import.meta.url);

function fail(message) {
  process.stderr.write(`deckprobe: ${message}\n`);
  process.exit(2);
}

const platform = currentPlatform();
if (!platform) {
  const supported = platforms.map((entry) => `${entry.os}-${entry.cpu}`);
  fail(
    `no prebuilt binary for ${process.platform}-${process.arch}.\n` +
      `Supported: ${[...new Set(supported)].join(", ")}.\n` +
      "Build from source instead: cargo install --git https://github.com/deckflow/deckprobe --locked deckprobe",
  );
}

const dependency = packageName(platform);
let binary;
try {
  binary = join(dirname(require.resolve(`${dependency}/package.json`)), "bin", binaryName(platform));
} catch {
  // The usual cause is an install that skipped optional dependencies.
  fail(
    `the platform package ${dependency} is not installed.\n` +
      "It ships as an optional dependency, so this happens after installing with --no-optional,\n" +
      "with a lockfile from a different platform, or behind a registry that filters optional deps.\n" +
      `Install it directly: npm install ${dependency}`,
  );
}

// stdio: "inherit" keeps --jsonl streaming, TTY detection, and back-pressure
// behaving exactly as they do for the native binary. Buffering here would
// silently change how large outputs and long-running pipes behave.
const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

const forwarded = ["SIGINT", "SIGTERM", "SIGHUP"];
const handlers = new Map();
for (const signal of forwarded) {
  const handler = () => {
    if (child.exitCode === null && child.signalCode === null) child.kill(signal);
  };
  handlers.set(signal, handler);
  process.on(signal, handler);
}

child.on("error", (error) => {
  fail(`cannot run ${binary}: ${error.message}`);
});

child.on("exit", (code, signal) => {
  for (const [name, handler] of handlers) process.off(name, handler);
  if (signal) {
    // Re-raise so the parent shell observes death-by-signal rather than a
    // plain exit status, which is what the native binary would produce.
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
