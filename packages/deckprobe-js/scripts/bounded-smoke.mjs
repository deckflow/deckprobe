import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { currentPlatform, packageName, binaryName } from "../bin/platforms.js";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const pkg = join(root, "packages/deckprobe-js");
const temp = mkdtempSync(join(tmpdir(), "deckprobe-bounded-"));
try {
  // A real package layout exercises Worker-relative assets and optional native resolution.
  const installed = join(temp, "node_modules/@deckflow/deckprobe");
  mkdirSync(installed, { recursive: true });
  for (const name of ["dist", "wasm", "bin", "package.json"]) cpSync(join(pkg, name), join(installed, name), { recursive: true });
  const { probeFile, version } = await import(pathToFileURL(join(installed, "dist/index.node.js")));
  assert.equal(await version(), "2.7.0");
  const file = join(root, "tests/fixtures/local/powerpoint-basic.pptx");
  const opts = { targets: ["powerpoint.slide_count", "powerpoint.smartart_data_part_count"], targetConfidence: { "powerpoint.slide_count": "exact" }, formatOptions: { "powerpoint.slide_count_path": "presentation-xml" } };
  let result = await probeFile(file, opts, {});
  assert.notEqual(result.status, "error");
  assert.equal(result.results["powerpoint.slide_count"].confidence, "exact");
  assert.equal(result.results["powerpoint.smartart_data_part_count"].value, 0);
  for (const runtime of [{ maxInputBytes: 1 }, { maxOutputBytes: 1 }, { deadlineMs: 1 }]) {
    const start = performance.now();
    result = await probeFile(file, opts, { ...runtime, backend: "wasm-worker" });
    assert.equal(result.error?.code, "BUDGET_EXCEEDED");
    assert.ok(performance.now() - start < 3000);
    await new Promise(r => setTimeout(r, 50));
  }
  const aborted = AbortSignal.abort();
  assert.equal((await probeFile(file, opts, { signal: aborted })).error?.code, "CANCELLED");
  assert.equal((await probeFile(file, opts, { maxInputBytes: -1 })).error?.code, "INVALID_REQUEST");
  // Install the same-version native optional package without registry access.
  const platform = currentPlatform();
  assert.ok(platform);
  const nativeDir = join(temp, "node_modules", packageName(platform));
  mkdirSync(join(nativeDir, "bin"), { recursive: true });
  writeFileSync(join(nativeDir, "package.json"), JSON.stringify({ name: packageName(platform), version: "2.7.0" }));
  cpSync(join(root, "target/release/deckprobe"), join(nativeDir, "bin", binaryName(platform)));
  const native = await probeFile(file, opts, { backend: "native" });
  const wasm = await probeFile(file, opts, { backend: "wasm-worker" });
  assert.deepEqual(native, wasm);
  assert.equal((await probeFile(file, opts, { backend: "native", maxInputBytes: 1 })).error?.code, "BUDGET_EXCEEDED");
  const custom = join(temp, "smartart.pptx");
  execFileSync("python3", ["-c", `import zipfile,sys
src,dst=sys.argv[1:]
with zipfile.ZipFile(src) as old,zipfile.ZipFile(dst,'w') as new:
 for item in old.infolist():
  data=old.read(item.filename)
  if item.filename=='[Content_Types].xml': data=data.replace(b'</Types>', b'<Override PartName="/custom/diagram.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml"/><Override PartName="/missing.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml"/></Types>')
  new.writestr(item,data)
 new.writestr('custom/diagram.xml','<data/>')
 new.writestr('ppt/diagrams/data999.xml','<not-declared/>')`, file, custom]);
  for (const backend of ["native", "wasm-worker"]) {
    result = await probeFile(custom, opts, { backend });
    assert.equal(result.results["powerpoint.smartart_data_part_count"].value, 1);
  }
  console.log("Bounded Node contract passed: fallback, native parity, caps, deadline, cancellation, SmartArt semantic inventory.");
} finally { rmSync(temp, { recursive: true, force: true }); }
