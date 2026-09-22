import type { ProbeResult } from "./types.js";

export interface ProbeRuntimeMetrics {
  backend: "native" | "wasm-worker";
  totalMs: number;
  terminationMs?: number;
  inputReadBytes?: number;
  inputLoadMs?: number;
  engineMs?: number;
}

/** Node-only host limits, separate from the engine's cooperative budget. */
export interface ProbeRuntimeOptions {
  deadlineMs?: number;
  maxInputBytes?: number;
  maxOutputBytes?: number;
  maxConcurrency?: number;
  backend?: "auto" | "native" | "wasm-worker";
  signal?: AbortSignal;
  onMetrics?: (metrics: ProbeRuntimeMetrics) => void;
}

export const NODE_PACKAGE_VERSION = "2.7.0";
export function runtimeError(code: string, message: string, exit_code = 4): ProbeResult {
  return { schema_version: 2, tool_version: NODE_PACKAGE_VERSION, status: "error", error: { code, message, exit_code } };
}
