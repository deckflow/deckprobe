import initWasm, {
  formats as wasmFormats,
  probe as wasmProbe,
  schema as wasmSchema,
  targets as wasmTargets,
  version as wasmVersion,
  type InitInput,
} from "../wasm/deckprobe_wasm.js";

import type {
  FormatsReport,
  ProbeCallOptions,
  ProbeInput,
  ProbeOptions,
  ProbeResult,
  TargetsReport,
} from "./types.js";

export type * from "./types.js";

let initialization: Promise<unknown> | undefined;

function normalizeError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

/** Initialize the WASM module once. Calling probe or discovery does this lazily. */
export function initDeckProbe(moduleOrPath?: InitInput): Promise<unknown> {
  initialization ??= initWasm(
    moduleOrPath === undefined ? undefined : { module_or_path: moduleOrPath },
  ).catch((error: unknown) => {
    initialization = undefined;
    throw normalizeError(error);
  });
  return initialization;
}

function engineOptions(options: ProbeCallOptions): ProbeOptions {
  const { name: _name, ...request } = options;
  return request;
}

function isFile(input: ProbeInput): input is File {
  return typeof File !== "undefined" && input instanceof File;
}

export async function inputBytes(input: ProbeInput): Promise<Uint8Array> {
  if (input instanceof Uint8Array) {
    return input;
  }
  if (input instanceof ArrayBuffer) {
    return new Uint8Array(input);
  }
  return new Uint8Array(await input.arrayBuffer());
}

export function inputName(input: ProbeInput, requestedName?: string): string {
  const name = requestedName ?? (isFile(input) ? input.name : undefined);
  if (!name || !name.includes(".")) {
    throw new TypeError(
      "DeckProbe requires options.name with a filename extension for non-File inputs",
    );
  }
  return name;
}

/** Probe a File or browser-owned byte buffer on the current thread. */
export async function probe(
  input: ProbeInput,
  options: ProbeCallOptions = {},
): Promise<ProbeResult> {
  const [bytes] = await Promise.all([inputBytes(input), initDeckProbe()]);
  try {
    return wasmProbe(
      inputName(input, options.name),
      bytes,
      engineOptions(options),
    ) as ProbeResult;
  } catch (error) {
    throw normalizeError(error);
  }
}

export async function formats(): Promise<FormatsReport> {
  await initDeckProbe();
  return wasmFormats() as FormatsReport;
}

export async function targets(format: string): Promise<TargetsReport | ProbeResult> {
  await initDeckProbe();
  return wasmTargets(format) as TargetsReport | ProbeResult;
}

export async function schema(): Promise<Record<string, unknown> | ProbeResult> {
  await initDeckProbe();
  return wasmSchema() as Record<string, unknown> | ProbeResult;
}

export async function version(): Promise<string> {
  await initDeckProbe();
  return wasmVersion();
}
