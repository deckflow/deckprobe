// Consumer-level guard for the bundler contract documented in README.md.
//
// The package resolves its WASM binary relative to its own wrapper, so a
// bundler that relocates the wrapper without the binary breaks initialization.
// Vite's dependency pre-bundling does exactly that in every version, and the
// symptom differs by version: Vite 4 answers 404 and WebAssembly reports
// "HTTP status code is not ok", while Vite 5+ serves the SPA fallback and
// WebAssembly rejects the HTML body instead. Asserting on one response status
// would miss the other, so each scenario is checked end to end: the probe must
// return a schema-v2 report with the page free of errors.

import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createServer } from "node:net";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const viteBin = resolve(packageDirectory, "node_modules/vite/bin/vite.js");
const expectedVersion = JSON.parse(
  readFileSync(resolve(packageDirectory, "package.json"), "utf8"),
).version;
const workspace = mkdtempSync(join(tmpdir(), "deckprobe-vite-"));
const consumer = join(workspace, "app");

/** Probe both entry points and report what a real application would observe. */
const mainThreadAndWorker = `
import { probe, version } from "@deckflow/deckprobe";
import { createDeckProbeWorker } from "@deckflow/deckprobe/worker";
import { minimalPdf } from "./fixture.js";

window.deckprobeSmoke = (async () => {
  const bytes = minimalPdf();
  const options = { targets: ["pdf.page_count"], level: "metadata" };
  const direct = await probe(bytes, { ...options, name: "minimal.pdf" });
  const worker = createDeckProbeWorker();
  try {
    const viaWorker = await worker.probe(new File([bytes], "minimal.pdf"), options);
    return { runtime: await version(), direct, viaWorker };
  } finally {
    worker.terminate();
  }
})().then((value) => ({ value }), (error) => ({ error: String(error?.stack ?? error) }));
`;

/**
 * The documented main-thread-only escape hatch. It deliberately omits the
 * Worker, which an explicit URL cannot reach; README.md says so and
 * dev-default below is what proves the Worker still needs optimizeDeps.
 */
const explicitWasmUrl = `
import { initDeckProbe, probe, version } from "@deckflow/deckprobe";
import wasmUrl from "@deckflow/deckprobe/wasm?url";
import { minimalPdf } from "./fixture.js";

window.deckprobeSmoke = (async () => {
  await initDeckProbe(wasmUrl);
  const direct = await probe(minimalPdf(), {
    name: "minimal.pdf",
    targets: ["pdf.page_count"],
    level: "metadata",
  });
  return { runtime: await version(), direct, wasmUrl };
})().then((value) => ({ value }), (error) => ({ error: String(error?.stack ?? error) }));
`;

const fixture = `
export function minimalPdf() {
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] /Contents 4 0 R >>",
    "<< /Length 0 >>\\nstream\\n\\nendstream",
  ];
  let source = "%PDF-1.4\\n";
  const offsets = [0];
  for (const [index, object] of objects.entries()) {
    offsets.push(source.length);
    source += \`\${index + 1} 0 obj\\n\${object}\\nendobj\\n\`;
  }
  const xref = source.length;
  source += \`xref\\n0 \${objects.length + 1}\\n0000000000 65535 f \\n\`;
  source += offsets
    .slice(1)
    .map((offset) => \`\${String(offset).padStart(10, "0")} 00000 n \\n\`)
    .join("");
  source += \`trailer\\n<< /Size \${objects.length + 1} /Root 1 0 R >>\\n\`;
  source += \`startxref\\n\${xref}\\n%%EOF\\n\`;
  return new TextEncoder().encode(source);
}
`;

const scenarios = [
  {
    name: "dev + optimizeDeps.exclude",
    entry: mainThreadAndWorker,
    config: `optimizeDeps: { exclude: ["@deckflow/deckprobe"] }`,
    mode: "dev",
    expectsWorker: true,
  },
  {
    name: "dev + explicit wasm URL",
    entry: explicitWasmUrl,
    config: "",
    mode: "dev",
    expectsWorker: false,
  },
  {
    name: "build + preview",
    entry: mainThreadAndWorker,
    config: "",
    mode: "preview",
    expectsWorker: true,
  },
];

async function freePort() {
  const server = createServer();
  await new Promise((ready, failed) => {
    server.once("error", failed);
    server.listen(0, "127.0.0.1", ready);
  });
  const { port } = server.address();
  await new Promise((closed) => server.close(closed));
  return port;
}

/** Vite prints its URL only after the server binds, so poll instead of racing. */
async function waitForServer(origin, child) {
  for (let attempt = 0; attempt < 150; attempt += 1) {
    if (child.exitCode !== null) {
      throw new Error(`vite exited early with code ${child.exitCode}`);
    }
    try {
      const response = await fetch(origin, { signal: AbortSignal.timeout(1000) });
      if (response.ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise((next) => setTimeout(next, 200));
  }
  throw new Error(`vite did not start at ${origin}`);
}

function writeProject({ entry, config }) {
  rmSync(consumer, { force: true, recursive: true });
  mkdirSync(consumer, { recursive: true });
  writeFileSync(
    join(consumer, "package.json"),
    JSON.stringify({ name: "deckprobe-vite-consumer", private: true, type: "module" }),
  );
  // A plain object rather than defineConfig(): the throwaway consumer has only
  // @deckflow/deckprobe installed, so the config must not import "vite".
  writeFileSync(join(consumer, "vite.config.js"), `export default { ${config} };\n`);
  writeFileSync(
    join(consumer, "index.html"),
    `<!doctype html><title>smoke</title><script type="module" src="/main.js"></script>`,
  );
  writeFileSync(join(consumer, "main.js"), entry);
  writeFileSync(join(consumer, "fixture.js"), fixture);
  cpSync(installed, join(consumer, "node_modules/@deckflow/deckprobe"), {
    recursive: true,
  });
}

function runVite(args, port) {
  // --host pins the bind address: Vite otherwise listens on "localhost", which
  // macOS resolves to ::1 first and leaves the 127.0.0.1 probe below failing.
  const child = spawn(
    process.execPath,
    [viteBin, ...args, "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
    { cwd: consumer, stdio: ["ignore", "pipe", "pipe"] },
  );
  const log = [];
  child.stdout.on("data", (chunk) => log.push(String(chunk)));
  child.stderr.on("data", (chunk) => log.push(String(chunk)));
  return { child, log };
}

// Pack the real tarball so the smoke test exercises the published "files" and
// "exports" surface rather than the working tree.
const packOutput = execFileSync(
  "npm",
  ["pack", "--silent", "--pack-destination", workspace],
  { cwd: packageDirectory, encoding: "utf8" },
);
// npm may interleave warnings with the filename, so pick the line by suffix.
const tarball = packOutput
  .split("\n")
  .map((line) => line.trim())
  .findLast((line) => line.endsWith(".tgz"));
assert.ok(tarball, `npm pack did not report a tarball:\n${packOutput}`);
execFileSync("tar", ["-xzf", join(workspace, tarball), "-C", workspace]);
const installed = join(workspace, "package");

const browser = await chromium.launch({ headless: true });
const failures = [];

try {
  for (const scenario of scenarios) {
    writeProject(scenario);
    const port = await freePort();
    const origin = `http://127.0.0.1:${port}`;
    let child;
    let log = [];
    const page = await browser.newPage();
    const pageErrors = [];
    const wasmResponses = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    page.on("console", (message) => {
      if (message.type() === "error") pageErrors.push(message.text());
    });
    page.on("response", (response) => {
      if (!response.url().includes(".wasm")) return;
      wasmResponses.push({
        status: response.status(),
        type: response.headers()["content-type"],
        url: response.url(),
      });
    });

    try {
      if (scenario.mode === "preview") {
        execFileSync(process.execPath, [viteBin, "build"], { cwd: consumer, stdio: "pipe" });
      }
      ({ child, log } = runVite([scenario.mode === "preview" ? "preview" : "dev"], port));
      await waitForServer(origin, child);
      await page.goto(origin, { waitUntil: "domcontentloaded" });
      await page.waitForFunction(() => Boolean(window.deckprobeSmoke), undefined, {
        timeout: 60_000,
      });
      // page.evaluate awaits the promise the entry module parked on window.
      const outcome = await page.evaluate(() => window.deckprobeSmoke);

      assert.equal(outcome.error, undefined, `${scenario.name}: ${outcome.error}`);
      assert.deepEqual(pageErrors, [], `${scenario.name} logged browser errors`);
      assert.equal(
        outcome.value.runtime,
        expectedVersion,
        `${scenario.name} reported a runtime version other than ${expectedVersion}`,
      );

      const reports = [outcome.value.direct];
      if (scenario.expectsWorker) reports.push(outcome.value.viaWorker);
      for (const report of reports) {
        assert.equal(report.schema_version, 2, `${scenario.name} returned a non-v2 report`);
        assert.notEqual(
          report.status,
          "error",
          `${scenario.name}: ${JSON.stringify(report.error)}`,
        );
        assert.equal(
          report.results["pdf.page_count"]?.value,
          1,
          `${scenario.name} did not resolve pdf.page_count`,
        );
      }

      // Every documented failure mode shows up here: Vite 4 answers 404, Vite
      // 5+ answers the SPA fallback as text/html, and a wrapper that never
      // reaches its binary requests nothing at all. The `?url` import also
      // produces a legitimate JavaScript module response next to the binary,
      // so require one real `application/wasm` body rather than rejecting it.
      assert.ok(wasmResponses.length > 0, `${scenario.name} never requested a .wasm file`);
      for (const response of wasmResponses) {
        assert.equal(response.status, 200, `${scenario.name}: ${response.url} was not 200`);
        assert.doesNotMatch(
          response.type ?? "",
          /^text\/html/,
          `${scenario.name}: ${response.url} served an HTML fallback instead of the binary`,
        );
      }
      assert.ok(
        wasmResponses.some((response) => /^application\/wasm/.test(response.type ?? "")),
        `${scenario.name} never received an application/wasm body`,
      );

      console.log(
        `ok  ${scenario.name} (${wasmResponses.length} wasm response(s), runtime ${outcome.value.runtime})`,
      );
    } catch (error) {
      const stderr = error.stderr ? String(error.stderr) : "";
      failures.push(`${scenario.name}: ${error.message}\n${stderr}${log.join("")}`);
      console.error(`FAIL ${scenario.name}`);
    } finally {
      await page.close();
      child?.kill("SIGTERM");
    }
  }
} finally {
  await browser.close();
  rmSync(workspace, { force: true, recursive: true });
}

if (failures.length) {
  console.error(`\n${failures.join("\n\n")}`);
  process.exit(1);
}

const viteVersion = JSON.parse(
  execFileSync(process.execPath, ["-p", "JSON.stringify(require('vite/package.json'))"], {
    cwd: packageDirectory,
    encoding: "utf8",
  }),
).version;
console.log(`Vite consumer smoke passed on vite ${viteVersion}`);
