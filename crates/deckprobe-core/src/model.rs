use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProbeLevel {
    Header,
    Metadata,
    Deep,
}

impl ProbeLevel {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "header" | "h" | "l0" | "0" => Some(Self::Header),
            "metadata" | "m" | "l1" | "1" => Some(Self::Metadata),
            "deep" | "d" | "l2" | "2" => Some(Self::Deep),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    None,
    Low,
    Medium,
    High,
    Exact,
}

impl Confidence {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "low" | "l" => Some(Self::Low),
            "medium" | "m" => Some(Self::Medium),
            "high" | "h" => Some(Self::High),
            "exact" | "x" => Some(Self::Exact),
            _ => None,
        }
    }

    /// Reported as `confidence_score`. This is `f64` rather than `f32` so the
    /// JSON is identical everywhere: serde_json prints the shortest round-trip
    /// form of an `f32` ("0.95"), but the WASM boundary widens `f32` to a JS
    /// number and would emit 0.949999988079071 for the same value.
    pub fn score(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::Low => 0.4,
            Self::Medium => 0.7,
            Self::High => 0.95,
            Self::Exact => 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    Resolved,
    Estimated,
    Planned,
    Unknown,
    Unsupported,
    Invalid,
    BudgetExceeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetScope {
    Common,
    Office,
    Format,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSpec {
    pub id: String,
    pub description: String,
    pub value_type: String,
    pub scope: TargetScope,
    pub min_level: ProbeLevel,
}

impl TargetSpec {
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        value_type: impl Into<String>,
        scope: TargetScope,
        min_level: ProbeLevel,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            value_type: value_type.into(),
            scope,
            min_level,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionSpec {
    pub key: String,
    pub description: String,
    pub value_type: String,
    pub default: String,
    pub allowed: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct FormatProfile {
    pub driver: &'static str,
    pub format: &'static str,
    pub profile: &'static str,
    pub mime_type: &'static str,
    pub extensions: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct Budget {
    pub max_physical_bytes: u64,
    pub max_expanded_bytes: u64,
    pub max_archive_entries: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BudgetOverrides {
    pub max_physical_bytes: Option<u64>,
    pub max_expanded_bytes: Option<u64>,
    pub max_archive_entries: Option<usize>,
    pub timeout_ms: Option<u64>,
}

impl BudgetOverrides {
    pub fn apply_to(&self, budget: &mut Budget) {
        if let Some(value) = self.max_physical_bytes {
            budget.max_physical_bytes = value;
        }
        if let Some(value) = self.max_expanded_bytes {
            budget.max_expanded_bytes = value;
        }
        if let Some(value) = self.max_archive_entries {
            budget.max_archive_entries = value;
        }
        if let Some(value) = self.timeout_ms {
            budget.timeout = Duration::from_millis(value);
        }
    }
}

/// Stable, source-independent request accepted by `deckprobe-engine`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProbeOptions {
    pub targets: Vec<String>,
    pub optional_targets: Vec<String>,
    pub level: ProbeLevel,
    pub minimum_confidence: Confidence,
    pub target_confidence: BTreeMap<String, Confidence>,
    pub allow_piggyback: bool,
    pub format_options: BTreeMap<String, String>,
    pub input_format: Option<String>,
    pub plan_only: bool,
    pub telemetry: bool,
    pub budget: BudgetOverrides,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            targets: vec!["@default".to_owned()],
            optional_targets: Vec::new(),
            level: ProbeLevel::Metadata,
            minimum_confidence: Confidence::High,
            target_confidence: BTreeMap::new(),
            allow_piggyback: true,
            format_options: BTreeMap::new(),
            input_format: None,
            plan_only: false,
            telemetry: false,
            budget: BudgetOverrides::default(),
        }
    }
}

impl Budget {
    pub fn for_level(level: ProbeLevel) -> Self {
        match level {
            ProbeLevel::Header => Self {
                // Modern iWork packages commonly have hundreds of entries even when
                // identity only needs the central directory and Document.iwa.
                max_physical_bytes: 4 * 1024 * 1024,
                max_expanded_bytes: 4 * 1024 * 1024,
                max_archive_entries: 4_096,
                timeout: Duration::from_millis(500),
            },
            ProbeLevel::Metadata => Self {
                max_physical_bytes: 16 * 1024 * 1024,
                max_expanded_bytes: 8 * 1024 * 1024,
                max_archive_entries: 10_000,
                timeout: Duration::from_millis(500),
            },
            ProbeLevel::Deep => Self {
                max_physical_bytes: 128 * 1024 * 1024,
                max_expanded_bytes: 256 * 1024 * 1024,
                max_archive_entries: 50_000,
                timeout: Duration::from_secs(5),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub targets: BTreeSet<String>,
    pub optional_targets: BTreeSet<String>,
    pub level: ProbeLevel,
    pub minimum_confidence: Confidence,
    pub target_confidence: BTreeMap<String, Confidence>,
    pub allow_piggyback: bool,
    pub format_options: BTreeMap<String, String>,
}

impl ProbeRequest {
    pub fn minimum_confidence_for(&self, target: &str) -> Confidence {
        self.target_confidence
            .get(target)
            .copied()
            .unwrap_or(self.minimum_confidence)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathDescriptor {
    pub id: String,
    pub targets: Vec<String>,
    pub min_level: ProbeLevel,
    pub confidence: Confidence,
    pub estimated_cost: u64,
}

impl PathDescriptor {
    pub fn new(
        id: impl Into<String>,
        targets: &[&str],
        min_level: ProbeLevel,
        confidence: Confidence,
        estimated_cost: u64,
    ) -> Self {
        Self {
            id: id.into(),
            targets: targets.iter().map(|value| (*value).to_owned()).collect(),
            min_level,
            confidence,
            estimated_cost,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub paths: Vec<String>,
    pub unresolved_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub piggyback_targets: Vec<String>,
    pub estimated_cost: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub target: String,
    pub status: TargetStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    pub confidence: Confidence,
    pub confidence_score: f64,
    pub path: String,
    pub source: String,
}

impl Evidence {
    pub fn resolved(
        target: impl Into<String>,
        value: impl Into<Value>,
        confidence: Confidence,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            target: target.into(),
            status: if confidence >= Confidence::High {
                TargetStatus::Resolved
            } else {
                TargetStatus::Estimated
            },
            value: Some(value.into()),
            confidence,
            confidence_score: confidence.score(),
            path: path.into(),
            source: source.into(),
        }
    }

    pub fn unresolved(
        target: impl Into<String>,
        status: TargetStatus,
        path: impl Into<String>,
    ) -> Self {
        Self {
            target: target.into(),
            status,
            value: None,
            confidence: Confidence::None,
            confidence_score: 0.0,
            path: path.into(),
            source: "none".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostSnapshot {
    pub physical_bytes_read: u64,
    pub expanded_bytes: u64,
    pub random_reads: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub level: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputReport {
    pub display_name: String,
    pub source_kind: String,
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverReport {
    pub id: String,
    pub profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub probe_level: ProbeLevel,
    pub paths: Vec<String>,
    pub estimated_cost: u64,
    pub actual_cost: CostSnapshot,
    pub unresolved_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub piggyback_targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub status: String,
    pub input: InputReport,
    pub driver: DriverReport,
    pub results: BTreeMap<String, Evidence>,
    pub execution: ExecutionReport,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn common_target_specs() -> Vec<TargetSpec> {
    use ProbeLevel::{Deep, Header, Metadata};
    use TargetScope::Common;
    vec![
        TargetSpec::new(
            "document.format",
            "Detected document family",
            "string",
            Common,
            Header,
        ),
        TargetSpec::new(
            "document.format_profile",
            "Detected suffix/profile",
            "string",
            Common,
            Header,
        ),
        TargetSpec::new(
            "document.mime_type",
            "Verified MIME type",
            "string",
            Common,
            Header,
        ),
        TargetSpec::new(
            "document.file_size",
            "Physical file size in bytes",
            "u64",
            Common,
            Header,
        ),
        TargetSpec::new(
            "document.extension",
            "Normalized input extension",
            "string",
            Common,
            Header,
        ),
        TargetSpec::new(
            "document.extension_matches",
            "Extension matches detected format",
            "bool",
            Common,
            Header,
        ),
        TargetSpec::new(
            "document.title",
            "Declared document title",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.subject",
            "Declared document subject",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.author",
            "Declared creator/author",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.keywords",
            "Declared keywords",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.description",
            "Declared description",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.created_at",
            "Declared creation time",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.modified_at",
            "Declared modification time",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.application",
            "Producer application",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.application_version",
            "Producer application version",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.language",
            "Declared document language",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "document.locale",
            "Declared document locale",
            "string|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "security.encrypted",
            "Container/content encryption detected",
            "bool",
            Common,
            Header,
        ),
        TargetSpec::new(
            "security.has_macros",
            "Embedded Office macro project detected",
            "bool",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "security.password_protected",
            "A non-empty password is required to read protected content",
            "bool|null",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "security.has_digital_signature",
            "Digital-signature structures are present; validity is not implied",
            "bool",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "security.signature_count",
            "Digital-signature structure count",
            "u64|null",
            Common,
            Deep,
        ),
        TargetSpec::new(
            "security.has_javascript",
            "Embedded active JavaScript is present",
            "bool",
            Common,
            Deep,
        ),
        TargetSpec::new(
            "security.has_external_relationships",
            "Document relationships point outside the package",
            "bool",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "security.has_embedded_files",
            "Embedded files or package objects are present",
            "bool",
            Common,
            Metadata,
        ),
        TargetSpec::new(
            "security.active_content_risk",
            "Rule-based active-content risk: none/low/medium/high/unknown",
            "string|null",
            Common,
            Deep,
        ),
        TargetSpec::new(
            "quality.corrupted",
            "Validated structural corruption was found; null means not fully checked",
            "bool|null",
            Common,
            Deep,
        ),
        TargetSpec::new(
            "quality.missing_assets",
            "Referenced internal assets are missing; null means not fully checked",
            "bool|null",
            Common,
            Deep,
        ),
    ]
}
