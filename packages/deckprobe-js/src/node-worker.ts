import { parentPort, workerData } from "node:worker_threads";
import { openSync, closeSync, fstatSync, readSync } from "node:fs";
import { basename } from "node:path";
import { performance } from "node:perf_hooks";
import { probe } from "./index.node.js";
import { runtimeError } from "./node-options.js";
import type { ProbeCallOptions, ProbeResult } from "./types.js";

const { path, options, maxInputBytes, maxOutputBytes, deadline } = workerData as {
  path: string; options: ProbeCallOptions; maxInputBytes: number; maxOutputBytes: number; deadline: number;
};
let inputReadBytes = 0, inputLoadMs = 0, engineMs = 0;
const check = () => {
  if (performance.timeOrigin + performance.now() >= deadline) throw new RangeError("Preflight deadline exceeded");
};
let report: ProbeResult;
try {
  const start = performance.now();
  check();
  const fd = openSync(path, "r");
  let bytes: Buffer;
  try {
    const before = fstatSync(fd);
    if (!before.isFile()) throw new Error("Input must be a regular file");
    if (before.size > maxInputBytes) throw new RangeError("Input exceeds maxInputBytes");
    bytes = Buffer.allocUnsafe(before.size);
    while (inputReadBytes < bytes.length) {
      check();
      const n = readSync(fd, bytes, inputReadBytes, Math.min(1024 * 1024, bytes.length - inputReadBytes), null);
      if (!n) throw new Error("Source changed during read");
      inputReadBytes += n;
    }
    const extra = readSync(fd, Buffer.alloc(1), 0, 1, null);
    const after = fstatSync(fd);
    if (extra || after.size !== before.size || after.mtimeMs !== before.mtimeMs || after.ctimeMs !== before.ctimeMs) {
      throw new Error("Source changed during read");
    }
  } finally { closeSync(fd); }
  inputLoadMs = performance.now() - start;
  check();
  const engineStart = performance.now();
  report = await probe(bytes, { ...options, name: options.name ?? basename(path), sourceKind: options.sourceKind ?? "local_file" });
  engineMs = performance.now() - engineStart;
} catch (error) {
  report = runtimeError(error instanceof RangeError ? "BUDGET_EXCEEDED" : "SOURCE_IO", error instanceof Error ? error.message : String(error), error instanceof RangeError ? 4 : 2);
}
let json = JSON.stringify(report);
if (Buffer.byteLength(json) > maxOutputBytes) json = JSON.stringify(runtimeError("BUDGET_EXCEEDED", "Report exceeds maxOutputBytes"));
parentPort!.postMessage({ json, metrics: { inputReadBytes, inputLoadMs, engineMs } });
parentPort!.close();
