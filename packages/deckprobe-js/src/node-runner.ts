import { Worker } from "node:worker_threads";
import { spawn, type ChildProcess } from "node:child_process";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
// The existing platform matrix also ships with the CLI launcher.
// @ts-expect-error Shared shipped JavaScript platform definitions.
import { currentPlatform, packageName, binaryName } from "../bin/platforms.js";
import { NODE_PACKAGE_VERSION, runtimeError, type ProbeRuntimeOptions, type ProbeRuntimeMetrics } from "./node-options.js";
import type { ProbeCallOptions, ProbeResult } from "./types.js";

const require = createRequire(import.meta.url);
let active = 0;
function nativeBinary(): string | undefined {
  const platform = currentPlatform();
  if (!platform) return;
  try {
    const manifest = require.resolve(`${packageName(platform)}/package.json`);
    if (JSON.parse(readFileSync(manifest, "utf8")).version !== NODE_PACKAGE_VERSION) return;
    return join(dirname(manifest), "bin", binaryName(platform));
  } catch { return; }
}

export function nativeArgs(file: string, options: ProbeCallOptions, maxInputBytes: number): string[] {
  const args = ["--max-input-bytes", String(maxInputBytes)];
  const add = (flag: string, value: unknown) => { if (value !== undefined) args.push(flag, String(value)); };
  if (options.targets?.length === 0) throw new TypeError("targets must not be empty in bounded mode");
  for (const target of options.targets ?? []) add("--targets", target);
  for (const target of options.optionalTargets ?? []) add("--optional-targets", target);
  add("--probe-level", options.level); add("--minimum-confidence", options.minimumConfidence);
  for (const [key, value] of Object.entries(options.targetConfidence ?? {})) add("--target-confidence", `${key}=${value}`);
  for (const [key, value] of Object.entries(options.formatOptions ?? {})) add("--format-option", `${key}=${value}`);
  add("--input-format", options.inputFormat);
  if (options.allowPiggyback === false) args.push("--no-piggyback");
  if (options.planOnly) args.push("--plan-only");
  if (options.telemetry) args.push("--telemetry");
  add("--probe-size", options.budget?.maxPhysicalBytes);
  add("--max-expanded-bytes", options.budget?.maxExpandedBytes);
  add("--max-archive-entries", options.budget?.maxArchiveEntries);
  add("--timeout-ms", options.budget?.timeoutMs);
  args.push("--", resolve(file));
  return args;
}

export async function boundedProbeFile(file: string, options: ProbeCallOptions, runtime: ProbeRuntimeOptions): Promise<ProbeResult> {
  const start = performance.now();
  const deadlineMs = runtime.deadlineMs ?? 2500;
  const maxInputBytes = runtime.maxInputBytes ?? 16 * 1024 * 1024;
  const maxOutputBytes = runtime.maxOutputBytes ?? 1024 * 1024;
  const maxConcurrency = runtime.maxConcurrency ?? 2;
  for (const [name, value] of Object.entries({ deadlineMs, maxInputBytes, maxOutputBytes, maxConcurrency })) {
    if (!Number.isSafeInteger(value) || value <= 0 || (name === "deadlineMs" && value > 2147483647)) return runtimeError("INVALID_REQUEST", `${name} must be a positive supported integer`, 1);
  }
  if (!["auto", "native", "wasm-worker"].includes(runtime.backend ?? "auto")) return runtimeError("INVALID_REQUEST", "Unknown backend", 1);
  if (runtime.signal?.aborted) return runtimeError("CANCELLED", "Preflight cancelled", 4);
  if (active >= maxConcurrency) return runtimeError("BUDGET_EXCEEDED", "Preflight concurrency limit reached");
  const deadline = performance.timeOrigin + start + deadlineMs;
  const request = { ...options, budget: { ...options.budget, timeoutMs: Math.min(options.budget?.timeoutMs ?? 1000, deadlineMs) } };
  // name changes routing in the bytes API; use that backend when an override is supplied.
  const binary = runtime.backend === "wasm-worker" || options.name !== undefined ? undefined : nativeBinary();
  if (runtime.backend === "native" && !binary) return runtimeError("INVALID_REQUEST", "Same-version native backend unavailable (name overrides require wasm-worker)", 1);
  active++;
  return new Promise((resolveResult) => {
    let settled = false, released = false;
    let unit: Worker | ChildProcess | undefined;
    let backend: ProbeRuntimeMetrics["backend"] = binary ? "native" : "wasm-worker";
    let responseMs = 0, stopStart = 0;
    let metrics: Partial<ProbeRuntimeMetrics> = {};
    const release = () => {
      if (released) return;
      released = true; active--;
      try { runtime.onMetrics?.({ backend, totalMs: responseMs || performance.now() - start, ...metrics, ...(stopStart ? { terminationMs: performance.now() - stopStart } : {}) }); } catch { /* Metrics cannot change the report. */ }
    };
    const stop = () => {
      stopStart ||= performance.now();
      if (unit instanceof Worker) void unit.terminate().catch(() => {});
      else if (unit) unit.kill("SIGKILL");
      else release();
    };
    const finish = (report: ProbeResult) => {
      if (settled) return;
      settled = true; responseMs = performance.now() - start;
      clearTimeout(timer); runtime.signal?.removeEventListener("abort", abort);
      stop(); resolveResult(report);
    };
    const abort = () => finish(runtimeError("CANCELLED", "Preflight cancelled"));
    const timer = setTimeout(() => finish(runtimeError("BUDGET_EXCEEDED", "Preflight deadline exceeded")), Math.max(0, deadlineMs - (performance.now() - start)));
    runtime.signal?.addEventListener("abort", abort, { once: true });
    const accept = (json: string) => {
      if (settled) return;
      try {
        if (Buffer.byteLength(json) > maxOutputBytes) return finish(runtimeError("BUDGET_EXCEEDED", "Report exceeds maxOutputBytes"));
        const report = JSON.parse(json) as ProbeResult;
        if (report.schema_version !== 2 || report.tool_version !== NODE_PACKAGE_VERSION || !["ok", "partial", "error"].includes(report.status)) throw new Error("Incompatible backend report");
        if (report.status !== "error" && options.sourceKind) report.input.source_kind = options.sourceKind;
        finish(report);
      } catch (error) { finish(runtimeError("PARSER_ERROR", String(error), 6)); }
    };
    const worker = () => {
      backend = "wasm-worker";
      try {
        const w = new Worker(new URL("./node-worker.js", import.meta.url), { workerData: { path: resolve(file), options: request, maxInputBytes, maxOutputBytes, deadline }, execArgv: [] });
        unit = w;
        w.once("message", (message) => { metrics = message.metrics; accept(message.json); });
        w.once("error", (error) => finish(runtimeError("PARSER_ERROR", error.message, 6)));
        w.once("exit", () => { if (!settled) finish(runtimeError("PARSER_ERROR", "Worker exited without a report", 6)); release(); });
      } catch (error) { finish(runtimeError("PARSER_ERROR", String(error), 6)); release(); }
    };
    if (runtime.signal?.aborted) { abort(); return; }
    if (!binary) { worker(); return; }
    try {
      const child = spawn(binary, nativeArgs(file, request, maxInputBytes), { stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
      unit = child;
      const chunks: Buffer[] = []; let size = 0, stderrSize = 0, failedStart = false;
      child.stdout!.on("data", (chunk: Buffer) => { size += chunk.length; if (size > maxOutputBytes) finish(runtimeError("BUDGET_EXCEEDED", "Report exceeds maxOutputBytes")); else chunks.push(chunk); });
      child.stderr!.on("data", (chunk: Buffer) => { stderrSize += chunk.length; if (stderrSize > maxOutputBytes) finish(runtimeError("BUDGET_EXCEEDED", "Backend diagnostics exceed maxOutputBytes")); });
      child.once("error", (error) => {
        failedStart = true;
        if (runtime.backend !== "native" && !child.pid && !settled && performance.timeOrigin + performance.now() < deadline) worker();
        else finish(runtimeError("SOURCE_IO", error.message, 2));
      });
      child.once("close", () => {
        if (failedStart && unit !== child) return;
        if (!settled) accept(Buffer.concat(chunks).toString("utf8"));
        release();
      });
    } catch (error) { finish(runtimeError("INVALID_REQUEST", String(error), 1)); release(); }
  });
}
