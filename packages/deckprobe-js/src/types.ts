export type ProbeLevel = "header" | "metadata" | "deep";
export type Confidence = "none" | "low" | "medium" | "high" | "exact";
/**
 * Where the probed bytes came from, as reported in `input.source_kind`.
 *
 * The built-in values are listed so they narrow in a `switch` or comparison,
 * while the trailing `(string & {})` keeps a custom {@link ProbeCallOptions.sourceKind}
 * assignable without widening the union to plain `string`.
 */
export type SourceKind =
  | "browser_bytes"
  | "node_bytes"
  | "local_file"
  | "stdin"
  | "jsonl_bytes"
  // eslint-disable-next-line @typescript-eslint/ban-types
  | (string & {});

/**
 * Compile-time guard for {@link SourceKind}.
 *
 * `node-smoke.mjs` and `browser-smoke.mjs` assert these exact strings at
 * runtime, so narrowing the union again -- as an earlier release did, declaring
 * only `"browser_bytes"` -- would republish types that reject a valid Node or
 * file report. `npm run check` fails if that happens.
 */
type AssertSourceKind<T extends SourceKind> = T;
type _RuntimeSourceKinds = AssertSourceKind<
  "browser_bytes" | "node_bytes" | "local_file"
>;

export type TargetStatus =
  | "resolved"
  | "estimated"
  | "planned"
  | "unknown"
  | "unsupported"
  | "invalid"
  | "budget_exceeded"
  | "failed";

export interface BudgetOverrides {
  maxPhysicalBytes?: number;
  maxExpandedBytes?: number;
  maxArchiveEntries?: number;
  timeoutMs?: number;
}

export interface ProbeOptions {
  targets?: string[];
  optionalTargets?: string[];
  level?: ProbeLevel;
  minimumConfidence?: Confidence;
  targetConfidence?: Record<string, Confidence>;
  allowPiggyback?: boolean;
  formatOptions?: Record<string, string>;
  inputFormat?: string;
  planOnly?: boolean;
  telemetry?: boolean;
  budget?: BudgetOverrides;
}

export interface ProbeCallOptions extends ProbeOptions {
  /** Required for bytes and Blob inputs; File inputs use File.name by default. */
  name?: string;
  /**
   * Value reported as `input.source_kind`. Defaults to `browser_bytes` in the
   * browser and `node_bytes` under Node; `probeFile()` reports `local_file`.
   * Set it when bytes reached you some other way and the report should say so.
   */
  sourceKind?: SourceKind;
}

/** Node's `Buffer` is a `Uint8Array`, so it is accepted wherever bytes are. */
export type ProbeInput = File | Blob | ArrayBuffer | Uint8Array;

export interface Evidence {
  target: string;
  status: TargetStatus;
  value?: unknown;
  confidence: Confidence;
  confidence_score: number;
  path: string;
  source: string;
}

export interface CostSnapshot {
  physical_bytes_read: number;
  expanded_bytes: number;
  random_reads: number;
  elapsed_ms?: number;
}

export interface Diagnostic {
  level: string;
  code: string;
  message: string;
}

export interface ProbeReport {
  schema_version: 2;
  tool_version: string;
  status: "ok" | "partial";
  input: {
    display_name: string;
    source_kind: SourceKind;
    file_size: number;
  };
  driver: {
    id: string;
    profile: string;
  };
  results: Record<string, Evidence>;
  execution: {
    probe_level: ProbeLevel;
    paths: string[];
    estimated_cost: number;
    actual_cost: CostSnapshot;
    unresolved_targets: string[];
    piggyback_targets?: string[];
  };
  diagnostics: Diagnostic[];
}

export interface ErrorReport {
  schema_version: 2;
  tool_version: string;
  status: "error";
  error: {
    code: string;
    message: string;
    exit_code: number;
  };
}

export type ProbeResult = ProbeReport | ErrorReport;

export interface FormatsReport {
  schema_version: 2;
  tool_version: string;
  status: "ok";
  formats: Array<{
    driver: string;
    profiles: string[];
    support: string;
  }>;
}

export interface TargetSpecReport {
  id: string;
  description: string;
  value_type: string;
  scope: "common" | "office" | "format";
  min_level: ProbeLevel;
  aliases: string[];
  schema: Record<string, unknown>;
  cost_class: "low" | "moderate" | "high";
  /** Whether the selected format has an execution path for this catalog target. */
  applicable: boolean;
  /** Probe levels at which the selected format can execute this target. */
  supported_levels: ProbeLevel[];
  selectors: string[];
}

export interface TargetsReport {
  schema_version: 2;
  tool_version: string;
  status: "ok";
  driver: string;
  profile: string;
  targets: TargetSpecReport[];
  selector_expansions: Record<string, Record<ProbeLevel, string[]>>;
  format_options: unknown[];
}
