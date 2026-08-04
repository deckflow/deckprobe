export type ProbeLevel = "header" | "metadata" | "deep";
export type Confidence = "none" | "low" | "medium" | "high" | "exact";
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
}

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
    source_kind: "browser_bytes";
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
