use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek};
use std::sync::Mutex;

use deckprobe_core::{
    Confidence, DeckProbeError, Evidence, ExecutionPlan, FormatDriver, FormatProfile, OptionSpec,
    PathDescriptor, ProbeContext, ProbeLevel, ProbeRequest, Result, TargetScope, TargetSpec,
    TargetStatus, common_target_specs, identity_evidence,
};
use plist::Value as PlistValue;
use serde_json::json;
use snap::raw::{Decoder as SnappyDecoder, decompress_len};
use zip::ZipArchive;

pub const KEYNOTE: FormatProfile = FormatProfile {
    driver: "keynote",
    format: "apple-iwork",
    profile: "key",
    mime_type: "application/x-iwork-keynote-sffkey",
    extensions: &["key"],
};

pub const NUMBERS: FormatProfile = FormatProfile {
    driver: "numbers",
    format: "apple-iwork",
    profile: "numbers",
    mime_type: "application/x-iwork-numbers-sffnumbers",
    extensions: &["numbers"],
};

pub const PAGES: FormatProfile = FormatProfile {
    driver: "pages",
    format: "apple-iwork",
    profile: "pages",
    mime_type: "application/x-iwork-pages-sffpages",
    extensions: &["pages"],
};

const DOCUMENT_IWA: &str = "Index/Document.iwa";
const PROPERTIES_PLIST: &str = "Metadata/Properties.plist";
const BUILD_HISTORY_PLIST: &str = "Metadata/BuildVersionHistory.plist";
const MAX_IWA_OBJECTS: usize = 1_000_000;

// Stable public archive types from Apple's modern iWork Protobuf schema.
const TSWP_STORAGE_ARCHIVE: [u64; 2] = [2001, 2005];
const TSWP_COMMENT_INFO_ARCHIVE: u64 = 2014;
const TSD_IMAGE_ARCHIVE: u64 = 3005;
const TSD_MOVIE_ARCHIVE: u64 = 3007;
const TSD_COMMENT_STORAGE_ARCHIVE: u64 = 3056;
const TSCH_CHART_DRAWABLE_ARCHIVE: u64 = 5021;
const TST_TABLE_INFO_ARCHIVE: u64 = 6000;
const TST_TABLE_MODEL_ARCHIVE: u64 = 6001;
const TST_TABLE_DATA_LIST_ARCHIVE: [u64; 2] = [6005, 6201];
const TST_WP_TABLE_INFO_ARCHIVE: u64 = 6007;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IworkKind {
    Keynote,
    Numbers,
    Pages,
}

impl IworkKind {
    fn profile(self) -> FormatProfile {
        match self {
            Self::Keynote => KEYNOTE,
            Self::Numbers => NUMBERS,
            Self::Pages => PAGES,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Keynote => "keynote",
            Self::Numbers => "numbers",
            Self::Pages => "pages",
        }
    }
}

pub struct IworkDriver {
    kind: IworkKind,
    profile: FormatProfile,
    prepared: Mutex<
        Option<IworkSession<deckprobe_core::BudgetedReader<deckprobe_core::BoxedProbeReader>>>,
    >,
}

impl IworkDriver {
    pub fn new(kind: IworkKind) -> Self {
        Self {
            kind,
            profile: kind.profile(),
            prepared: Mutex::new(None),
        }
    }

    /// Opens and validates the package generation once during dispatch, then
    /// hands the live ZIP session to execution. This avoids parsing the central
    /// directory twice for the common detect-then-probe flow.
    pub fn open(context: &ProbeContext, kind: IworkKind) -> Result<Self> {
        let session = IworkSession::open(context)?;
        Ok(Self {
            kind,
            profile: kind.profile(),
            prepared: Mutex::new(Some(session)),
        })
    }

    fn format_targets(&self) -> Vec<TargetSpec> {
        use ProbeLevel::Metadata;
        use TargetScope::Format;

        match self.kind {
            IworkKind::Keynote => vec![
                TargetSpec::new(
                    "keynote.slide_count",
                    "Slide component count",
                    "u64",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "keynote.master_slide_count",
                    "Master/template slide component count",
                    "u64",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "keynote.table_component_count",
                    "Table tile component count",
                    "u64",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "keynote.slide_size",
                    "Presentation canvas size decoded from KN.ShowArchive",
                    "object|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "keynote.aspect_ratio",
                    "Presentation canvas aspect ratio",
                    "object|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "keynote.orientation",
                    "Presentation canvas orientation",
                    "string|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "keynote.hidden_slide_count",
                    "Hidden logical slide count from KN.SlideNodeArchive",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "keynote.slides_with_notes_count",
                    "Logical slide count carrying presenter notes",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "keynote.slides_with_builds_count",
                    "Logical slide count carrying builds",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "keynote.slides_with_transitions_count",
                    "Logical slide count carrying transitions",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "keynote.table_count",
                    "Referenced logical table model count",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
            ],
            IworkKind::Numbers => vec![
                TargetSpec::new(
                    "numbers.sheet_count",
                    "Logical sheet count from the TN document archive",
                    "u64|null",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "numbers.sheet_names",
                    "Ordered logical sheet names from TN sheet archives",
                    "array<string>|null",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "numbers.table_component_count",
                    "Table tile component count",
                    "u64",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "numbers.table_count",
                    "Referenced logical table model count",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "numbers.table_dimensions",
                    "Logical table names and row/column dimensions",
                    "array<object>",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "numbers.hidden_row_count",
                    "Aggregate hidden row count across logical tables",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "numbers.hidden_column_count",
                    "Aggregate hidden column count across logical tables",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "numbers.filtered_row_count",
                    "Aggregate filtered row count across logical tables",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "numbers.formula_definition_count",
                    "Persisted formula-table definition count",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
            ],
            IworkKind::Pages => vec![
                TargetSpec::new(
                    "pages.table_component_count",
                    "Table tile component count",
                    "u64",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "pages.section_count",
                    "Persisted document section count",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.section_names",
                    "Persisted named document sections",
                    "array<string>",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.page_size",
                    "Document page size decoded from TP.DocumentArchive",
                    "object|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.aspect_ratio",
                    "Document page aspect ratio",
                    "object|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.orientation",
                    "Document page orientation",
                    "string|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.change_tracking_enabled",
                    "Document change-tracking flag",
                    "bool",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.body_text_length",
                    "Body text length in UTF-16 code units",
                    "u64|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.body_paragraph_break_count",
                    "Body text newline count in the TSWP storage",
                    "u64|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.cached_page_count",
                    "Last persisted layout page count, when present",
                    "u64|null",
                    Format,
                    ProbeLevel::Deep,
                ),
                TargetSpec::new(
                    "pages.table_count",
                    "Referenced logical table model count",
                    "u64",
                    Format,
                    ProbeLevel::Deep,
                ),
            ],
        }
    }

    fn deep_targets(&self) -> Vec<&'static str> {
        let mut targets = vec![
            "iwork.archive_object_count",
            "iwork.message_type_counts",
            "iwork.object_type_counts",
            "iwork.all_iwa_valid",
            "quality.corrupted",
        ];
        targets.extend(match self.kind {
            IworkKind::Keynote => vec![
                "keynote.slide_size",
                "keynote.aspect_ratio",
                "keynote.orientation",
                "keynote.hidden_slide_count",
                "keynote.slides_with_notes_count",
                "keynote.slides_with_builds_count",
                "keynote.slides_with_transitions_count",
                "keynote.table_count",
            ],
            IworkKind::Numbers => vec![
                "numbers.table_count",
                "numbers.table_dimensions",
                "numbers.hidden_row_count",
                "numbers.hidden_column_count",
                "numbers.filtered_row_count",
                "numbers.formula_definition_count",
            ],
            IworkKind::Pages => vec![
                "pages.section_count",
                "pages.section_names",
                "pages.page_size",
                "pages.aspect_ratio",
                "pages.orientation",
                "pages.change_tracking_enabled",
                "pages.body_text_length",
                "pages.body_paragraph_break_count",
                "pages.cached_page_count",
                "pages.table_count",
            ],
        });
        targets
    }

    fn plist_targets(&self) -> Vec<&'static str> {
        vec![
            "document.application_version",
            "document.language",
            "document.locale",
            "iwork.file_format_version",
            "iwork.producer_build",
            "iwork.is_multi_page",
            "iwork.has_external_or_missing_data",
        ]
    }

    fn inventory_targets(&self) -> Vec<&'static str> {
        let mut targets = vec![
            "document.application",
            "security.has_macros",
            "security.password_protected",
            "iwork.package_entry_count",
            "iwork.iwa_entry_count",
            "iwork.data_asset_count",
            "iwork.data_asset_bytes",
            "iwork.asset_type_counts",
            "iwork.has_preview",
            "iwork.preview_count",
        ];
        targets.extend(match self.kind {
            IworkKind::Keynote => vec![
                "keynote.slide_count",
                "keynote.master_slide_count",
                "keynote.table_component_count",
            ],
            IworkKind::Numbers => vec![
                "numbers.sheet_count",
                "numbers.sheet_names",
                "numbers.table_component_count",
            ],
            IworkKind::Pages => vec!["pages.table_component_count"],
        });
        targets
    }

    fn preview_targets(&self) -> Vec<&'static str> {
        vec!["iwork.preview_dimensions"]
    }

    fn integrity_targets(&self) -> Vec<&'static str> {
        vec!["iwork.all_iwa_valid", "quality.corrupted"]
    }
}

impl FormatDriver for IworkDriver {
    fn id(&self) -> &'static str {
        self.kind.label()
    }

    fn profile(&self) -> &FormatProfile {
        &self.profile
    }

    fn targets(&self) -> Vec<TargetSpec> {
        let mut targets = common_target_specs();
        targets.extend(iwork_target_specs());
        targets.extend(self.format_targets());
        targets
    }

    fn options(&self) -> Vec<OptionSpec> {
        Vec::new()
    }

    fn default_targets(&self, level: ProbeLevel) -> BTreeSet<String> {
        let mut targets = identity_targets()
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<BTreeSet<_>>();
        if level >= ProbeLevel::Metadata {
            targets.extend(self.plist_targets().into_iter().map(str::to_owned));
            targets.extend(self.inventory_targets().into_iter().map(str::to_owned));
            // 1.1 additions stay opt-in so the established @default report is
            // byte-compatible apart from the tool version.
            targets.remove("security.password_protected");
            targets.remove("iwork.preview_count");
            targets.remove("document.language");
            targets.remove("document.locale");
            targets.remove("iwork.producer_build");
            targets.remove("iwork.has_external_or_missing_data");
            targets.remove("iwork.data_asset_bytes");
            targets.remove("iwork.asset_type_counts");
        }
        targets
    }

    fn paths(&self, _request: &ProbeRequest) -> Result<Vec<PathDescriptor>> {
        let plist_targets = self.plist_targets();
        let inventory_targets = self.inventory_targets();
        let preview_targets = self.preview_targets();
        let integrity_targets = self.integrity_targets();
        let deep_targets = self.deep_targets();
        Ok(vec![
            PathDescriptor::new(
                "iwork.identity",
                identity_targets(),
                ProbeLevel::Header,
                Confidence::Exact,
                3,
            ),
            PathDescriptor::new(
                "iwork.package_metadata",
                &plist_targets,
                ProbeLevel::Metadata,
                Confidence::High,
                4,
            ),
            PathDescriptor::new(
                "iwork.package_inventory",
                &inventory_targets,
                ProbeLevel::Metadata,
                Confidence::Exact,
                8,
            ),
            PathDescriptor::new(
                "iwork.preview_metadata",
                &preview_targets,
                ProbeLevel::Metadata,
                Confidence::Exact,
                5,
            ),
            PathDescriptor::new(
                "iwork.iwa_integrity",
                &integrity_targets,
                ProbeLevel::Deep,
                Confidence::Exact,
                40,
            ),
            PathDescriptor::new(
                "iwork.iwa_objects",
                &deep_targets,
                ProbeLevel::Deep,
                Confidence::Exact,
                42,
            ),
        ])
    }

    fn validate_options(&self, request: &ProbeRequest) -> Result<()> {
        if let Some((key, _)) = request.format_options.first_key_value() {
            return Err(DeckProbeError::InvalidRequest(format!(
                "unknown {} option: {key}",
                self.kind.label()
            )));
        }
        Ok(())
    }

    fn execute(
        &self,
        context: &mut ProbeContext,
        request: &ProbeRequest,
        plan: &ExecutionPlan,
    ) -> Result<Vec<Evidence>> {
        let mut session = self
            .prepared
            .lock()
            .map_err(|_| DeckProbeError::Parser("prepared iWork session lock poisoned".to_owned()))?
            .take()
            .map(Ok)
            .unwrap_or_else(|| IworkSession::open(context))?;
        session.validate_profile(context, self.kind)?;
        let mut output = Vec::new();
        for path in &plan.paths {
            context.check_time()?;
            match path.as_str() {
                "iwork.identity" => {
                    output.extend(identity_path_evidence(context, &self.profile, self.kind));
                }
                "iwork.package_metadata" => {
                    output.extend(session.plist_evidence(context, path)?);
                }
                "iwork.package_inventory" => {
                    let include_data_bytes = request.targets.contains("iwork.data_asset_bytes")
                        || plan
                            .piggyback_targets
                            .iter()
                            .any(|target| target == "iwork.data_asset_bytes");
                    output.extend(session.inventory_evidence(
                        context,
                        self.kind,
                        path,
                        include_data_bytes,
                    )?);
                }
                "iwork.preview_metadata" => {
                    output.extend(session.preview_evidence(context, path)?);
                }
                "iwork.iwa_integrity" => {
                    output.extend(session.integrity_evidence(context, path)?);
                }
                "iwork.iwa_objects" => {
                    output.extend(session.deep_evidence(context, self.kind, path)?);
                }
                _ => {}
            }
        }
        Ok(output)
    }
}

/// Checks the ZIP generation marker before planning. This deliberately does not
/// decode the document; the selected driver performs full IWA/profile validation.
pub fn validate_modern_iwork_package(context: &ProbeContext) -> Result<()> {
    let reader = context.open_budgeted_reader()?;
    let archive = ZipArchive::new(reader)
        .map_err(|error| map_zip_error(context, "invalid iWork ZIP container", error))?;
    check_entry_budget(context, archive.len())?;
    let names = archive.file_names().collect::<BTreeSet<_>>();
    validate_generation(&names)?;
    context.check_time()
}

fn iwork_target_specs() -> Vec<TargetSpec> {
    use ProbeLevel::{Deep, Header, Metadata};
    use TargetScope::Format;
    vec![
        TargetSpec::new(
            "iwork.document_kind",
            "Apple iWork family: keynote/numbers/pages",
            "string",
            Format,
            Header,
        ),
        TargetSpec::new(
            "iwork.file_format_version",
            "IWA serialization version from Properties.plist",
            "string|null",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.producer_build",
            "Latest producer build entry from BuildVersionHistory.plist",
            "string|null",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.package_entry_count",
            "ZIP package entry count",
            "u64",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.iwa_entry_count",
            "IWA component entry count",
            "u64",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.data_asset_count",
            "Embedded Data/ asset count",
            "u64",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.data_asset_bytes",
            "Total uncompressed bytes declared for Data/ assets",
            "u64",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.asset_type_counts",
            "Data/ asset counts grouped as image, audio, video, font, and other",
            "object",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.has_preview",
            "Package contains a generated preview image",
            "bool",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.preview_count",
            "Generated preview image count in the package",
            "u64",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.preview_dimensions",
            "Preview image dimensions keyed by package entry name",
            "object",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.is_multi_page",
            "Properties.plist multi-page flag",
            "bool|null",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.has_external_or_missing_data",
            "Properties flag combining external, missing, or unmaterialized remote data",
            "bool|null",
            Format,
            Metadata,
        ),
        TargetSpec::new(
            "iwork.all_iwa_valid",
            "Every IWA entry passed Snappy framing and protobuf archive validation",
            "bool",
            Format,
            Deep,
        ),
        TargetSpec::new(
            "iwork.archive_object_count",
            "Total decoded Protobuf archive object count across all IWA entries",
            "u64",
            Format,
            Deep,
        ),
        TargetSpec::new(
            "iwork.message_type_counts",
            "Decoded archive object counts keyed by numeric IWA message type",
            "object",
            Format,
            Deep,
        ),
        TargetSpec::new(
            "iwork.object_type_counts",
            "Decoded semantic object counts for text, image, movie, chart, table, and comment archive classes",
            "object",
            Format,
            Deep,
        ),
    ]
}

fn identity_targets() -> &'static [&'static str] {
    &[
        "document.format",
        "document.format_profile",
        "document.mime_type",
        "document.file_size",
        "document.extension",
        "document.extension_matches",
        "security.encrypted",
        "iwork.document_kind",
    ]
}

fn identity_path_evidence(
    context: &ProbeContext,
    profile: &FormatProfile,
    kind: IworkKind,
) -> Vec<Evidence> {
    let mut evidence = identity_evidence(context, profile, "iwork.identity");
    evidence.extend([
        Evidence::resolved(
            "security.encrypted",
            false,
            Confidence::Exact,
            "iwork.identity",
            "validated readable IWA package",
        ),
        Evidence::resolved(
            "iwork.document_kind",
            kind.label(),
            Confidence::Exact,
            "iwork.identity",
            "IWA root object + package component inventory",
        ),
    ]);
    evidence
}

struct IworkSession<R: Read + Seek> {
    archive: ZipArchive<R>,
    entry_names: Vec<String>,
    properties: Option<IworkProperties>,
    producer_build: Option<Option<String>>,
    document_objects: Option<Vec<ArchiveObject>>,
    all_objects: Option<Vec<ArchiveObject>>,
}

impl IworkSession<deckprobe_core::BudgetedReader<deckprobe_core::BoxedProbeReader>> {
    fn open(context: &ProbeContext) -> Result<Self> {
        let reader = context.open_budgeted_reader()?;
        let archive = ZipArchive::new(reader)
            .map_err(|error| map_zip_error(context, "invalid iWork ZIP container", error))?;
        check_entry_budget(context, archive.len())?;
        let entry_names = archive.file_names().map(str::to_owned).collect::<Vec<_>>();
        validate_generation(&entry_names.iter().map(String::as_str).collect())?;
        context.check_time()?;
        Ok(Self {
            archive,
            entry_names,
            properties: None,
            producer_build: None,
            document_objects: None,
            all_objects: None,
        })
    }
}

impl<R: Read + Seek> IworkSession<R> {
    fn validate_profile(&mut self, context: &ProbeContext, kind: IworkKind) -> Result<()> {
        if !self.has_entry(PROPERTIES_PLIST) {
            return Err(DeckProbeError::MalformedInput(format!(
                "modern .{} package is missing {PROPERTIES_PLIST}",
                kind.profile().profile
            )));
        }
        let object_types = self
            .document_objects(context)?
            .iter()
            .map(|object| object.message_type)
            .collect::<BTreeSet<_>>();
        let matches = match kind {
            IworkKind::Keynote => {
                object_types.contains(&1)
                    && self
                        .entry_names
                        .iter()
                        .any(|name| is_component(name, "Index/Slide"))
            }
            IworkKind::Numbers => {
                object_types.contains(&1)
                    && object_types.contains(&2)
                    && !object_types.contains(&10_000)
                    && !self
                        .entry_names
                        .iter()
                        .any(|name| is_component(name, "Index/Slide"))
                    && self
                        .entry_names
                        .iter()
                        .any(|name| name.starts_with("Index/Tables/"))
            }
            IworkKind::Pages => object_types.contains(&10_000),
        };
        if !matches {
            return Err(DeckProbeError::MalformedInput(format!(
                "IWA root objects do not match the .{} profile",
                kind.profile().profile
            )));
        }
        context.check_time()
    }

    fn plist_evidence(&mut self, context: &ProbeContext, path: &str) -> Result<Vec<Evidence>> {
        let properties = self.properties(context)?.clone();
        let producer_build = self.producer_build(context)?.map(str::to_owned);
        Ok(vec![
            optional_string_evidence(
                "iwork.file_format_version",
                properties.file_format_version.as_deref(),
                path,
                PROPERTIES_PLIST,
            ),
            optional_string_evidence(
                "document.application_version",
                producer_build.as_deref(),
                path,
                BUILD_HISTORY_PLIST,
            ),
            optional_string_evidence(
                "iwork.producer_build",
                producer_build.as_deref(),
                path,
                BUILD_HISTORY_PLIST,
            ),
            optional_string_evidence(
                "document.language",
                properties.language.as_deref(),
                path,
                PROPERTIES_PLIST,
            ),
            optional_string_evidence(
                "document.locale",
                properties.locale.as_deref(),
                path,
                PROPERTIES_PLIST,
            ),
            optional_bool_evidence(
                "iwork.is_multi_page",
                properties.is_multi_page,
                path,
                PROPERTIES_PLIST,
            ),
            optional_bool_evidence(
                "iwork.has_external_or_missing_data",
                properties.has_external_or_missing_data,
                path,
                PROPERTIES_PLIST,
            ),
        ])
    }

    fn inventory_evidence(
        &mut self,
        context: &ProbeContext,
        kind: IworkKind,
        path: &str,
        include_data_bytes: bool,
    ) -> Result<Vec<Evidence>> {
        let iwa_count = self
            .entry_names
            .iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(".iwa"))
            .count();
        let data_count = self
            .entry_names
            .iter()
            .filter(|name| name.starts_with("Data/") && !name.ends_with('/'))
            .count();
        let data_bytes = include_data_bytes
            .then(|| self.data_asset_bytes(context))
            .transpose()?;
        let asset_type_counts = asset_type_counts(&self.entry_names);
        let preview_count = self
            .entry_names
            .iter()
            .filter(|name| is_preview_entry(name))
            .count();

        let mut evidence = vec![
            Evidence::resolved(
                "document.application",
                application_name(kind),
                Confidence::Exact,
                path,
                "validated IWA family",
            ),
            Evidence::resolved(
                "security.has_macros",
                false,
                Confidence::Exact,
                path,
                "modern IWA format capability",
            ),
            Evidence::resolved(
                "security.password_protected",
                false,
                Confidence::Exact,
                path,
                "readable validated IWA package",
            ),
            Evidence::resolved(
                "iwork.package_entry_count",
                json!(self.entry_names.len()),
                Confidence::Exact,
                path,
                "ZIP central directory",
            ),
            Evidence::resolved(
                "iwork.iwa_entry_count",
                json!(iwa_count),
                Confidence::Exact,
                path,
                "ZIP central directory",
            ),
            Evidence::resolved(
                "iwork.data_asset_count",
                json!(data_count),
                Confidence::Exact,
                path,
                "ZIP central directory",
            ),
            Evidence::resolved(
                "iwork.asset_type_counts",
                json!(asset_type_counts),
                Confidence::Exact,
                path,
                "Data/ filename extensions",
            ),
            Evidence::resolved(
                "iwork.has_preview",
                preview_count > 0,
                Confidence::Exact,
                path,
                "ZIP central directory",
            ),
            Evidence::resolved(
                "iwork.preview_count",
                json!(preview_count),
                Confidence::Exact,
                path,
                "ZIP central directory",
            ),
        ];
        if let Some(data_bytes) = data_bytes {
            evidence.push(Evidence::resolved(
                "iwork.data_asset_bytes",
                json!(data_bytes),
                Confidence::Exact,
                path,
                "ZIP central directory uncompressed sizes",
            ));
        }
        match kind {
            IworkKind::Keynote => evidence.extend([
                count_evidence(
                    "keynote.slide_count",
                    count_components(&self.entry_names, "Index/Slide"),
                    path,
                ),
                count_evidence(
                    "keynote.master_slide_count",
                    count_components(&self.entry_names, "Index/MasterSlide")
                        + count_components(&self.entry_names, "Index/TemplateSlide"),
                    path,
                ),
                count_evidence(
                    "keynote.table_component_count",
                    count_components(&self.entry_names, "Index/Tables/Tile"),
                    path,
                ),
            ]),
            IworkKind::Numbers => {
                let sheets = numbers_sheets(self.document_objects(context)?);
                match sheets {
                    Some(sheets) => {
                        evidence.push(Evidence::resolved(
                            "numbers.sheet_count",
                            json!(sheets.len()),
                            Confidence::Exact,
                            path,
                            "TN.DocumentArchive sheet references",
                        ));
                        evidence.push(Evidence::resolved(
                            "numbers.sheet_names",
                            json!(sheets),
                            Confidence::Exact,
                            path,
                            "TN.SheetArchive names",
                        ));
                    }
                    None => {
                        evidence.push(Evidence::unresolved(
                            "numbers.sheet_count",
                            TargetStatus::Unknown,
                            path,
                        ));
                        evidence.push(Evidence::unresolved(
                            "numbers.sheet_names",
                            TargetStatus::Unknown,
                            path,
                        ));
                    }
                }
                evidence.push(count_evidence(
                    "numbers.table_component_count",
                    count_components(&self.entry_names, "Index/Tables/Tile"),
                    path,
                ));
            }
            IworkKind::Pages => evidence.push(count_evidence(
                "pages.table_component_count",
                count_components(&self.entry_names, "Index/Tables/Tile"),
                path,
            )),
        }
        Ok(evidence)
    }

    fn preview_evidence(&mut self, context: &ProbeContext, path: &str) -> Result<Vec<Evidence>> {
        let preview_names = self
            .entry_names
            .iter()
            .filter(|name| is_preview_entry(name))
            .cloned()
            .collect::<Vec<_>>();
        let mut dimensions = BTreeMap::new();
        for name in preview_names {
            let bytes = self.read_entry(context, &name)?;
            let value = image_dimensions(&bytes)
                .map(|(width, height)| json!({"width": width, "height": height}))
                .unwrap_or(serde_json::Value::Null);
            dimensions.insert(name, value);
        }
        Ok(vec![Evidence::resolved(
            "iwork.preview_dimensions",
            json!(dimensions),
            Confidence::Exact,
            path,
            "PNG/JPEG preview headers",
        )])
    }

    fn deep_evidence(
        &mut self,
        context: &ProbeContext,
        kind: IworkKind,
        path: &str,
    ) -> Result<Vec<Evidence>> {
        let objects = self.all_objects(context)?;
        let mut evidence = common_deep_evidence(objects, path);
        evidence.extend(match kind {
            IworkKind::Keynote => keynote_deep_evidence(objects, path),
            IworkKind::Numbers => numbers_deep_evidence(objects, path),
            IworkKind::Pages => pages_deep_evidence(objects, path),
        });
        evidence.extend(integrity_result_evidence(path));
        Ok(evidence)
    }

    fn integrity_evidence(&mut self, context: &ProbeContext, path: &str) -> Result<Vec<Evidence>> {
        self.all_objects(context)?;
        Ok(integrity_result_evidence(path))
    }

    fn has_entry(&self, name: &str) -> bool {
        self.entry_names.iter().any(|candidate| candidate == name)
    }

    fn data_asset_bytes(&mut self, context: &ProbeContext) -> Result<u64> {
        let mut total = 0u64;
        for index in 0..self.archive.len() {
            context.check_time()?;
            let entry = self
                .archive
                .by_index(index)
                .map_err(|error| map_zip_error(context, "invalid iWork ZIP entry", error))?;
            if entry.name().starts_with("Data/") && !entry.name().ends_with('/') {
                total = total.checked_add(entry.size()).ok_or_else(|| {
                    DeckProbeError::MalformedInput("Data/ asset byte count overflow".to_owned())
                })?;
            }
        }
        Ok(total)
    }

    fn read_entry(&mut self, context: &ProbeContext, name: &str) -> Result<Vec<u8>> {
        let mut entry = self.archive.by_name(name).map_err(|error| {
            DeckProbeError::MalformedInput(format!("missing or unreadable {name}: {error}"))
        })?;
        let declared_size = entry.size();
        context.record_expanded(declared_size)?;
        if declared_size > context.budget().max_expanded_bytes {
            return Err(DeckProbeError::BudgetExceeded(format!(
                "entry {name} size {declared_size} exceeds expanded budget"
            )));
        }
        let mut bytes = Vec::with_capacity(declared_size.min(1024 * 1024) as usize);
        entry.read_to_end(&mut bytes).map_err(|error| {
            if let Some(message) = context.budget_failure() {
                DeckProbeError::BudgetExceeded(message)
            } else if error.to_string().contains("deckprobe") {
                DeckProbeError::BudgetExceeded(error.to_string())
            } else {
                DeckProbeError::MalformedInput(format!("cannot read {name}: {error}"))
            }
        })?;
        context.check_time()?;
        Ok(bytes)
    }

    fn properties(&mut self, context: &ProbeContext) -> Result<&IworkProperties> {
        if self.properties.is_none() {
            let bytes = self.read_entry(context, PROPERTIES_PLIST)?;
            let value = PlistValue::from_reader(Cursor::new(bytes)).map_err(|error| {
                DeckProbeError::MalformedInput(format!("invalid {PROPERTIES_PLIST}: {error}"))
            })?;
            let dictionary = value.as_dictionary().ok_or_else(|| {
                DeckProbeError::MalformedInput(format!(
                    "{PROPERTIES_PLIST} root is not a dictionary"
                ))
            })?;
            self.properties = Some(IworkProperties {
                file_format_version: dictionary
                    .get("fileFormatVersion")
                    .and_then(PlistValue::as_string)
                    .map(str::to_owned),
                is_multi_page: dictionary
                    .get("isMultiPage")
                    .and_then(PlistValue::as_boolean),
                has_external_or_missing_data: dictionary
                    .get("hasExternalReferenceOrMissingOrUnmaterializedRemoteData")
                    .and_then(PlistValue::as_boolean),
                language: dictionary
                    .get("language")
                    .and_then(PlistValue::as_string)
                    .map(str::to_owned),
                locale: dictionary
                    .get("locale")
                    .and_then(PlistValue::as_string)
                    .map(str::to_owned),
            });
        }
        Ok(self.properties.as_ref().expect("properties initialized"))
    }

    fn producer_build(&mut self, context: &ProbeContext) -> Result<Option<&str>> {
        if self.producer_build.is_none() {
            let build = if self.has_entry(BUILD_HISTORY_PLIST) {
                let bytes = self.read_entry(context, BUILD_HISTORY_PLIST)?;
                let value = PlistValue::from_reader(Cursor::new(bytes)).map_err(|error| {
                    DeckProbeError::MalformedInput(format!(
                        "invalid {BUILD_HISTORY_PLIST}: {error}"
                    ))
                })?;
                latest_producer_build(&value)
            } else {
                None
            };
            self.producer_build = Some(build);
        }
        Ok(self.producer_build.as_ref().and_then(Option::as_deref))
    }

    fn document_objects(&mut self, context: &ProbeContext) -> Result<&[ArchiveObject]> {
        if self.document_objects.is_none() {
            let raw = self.read_entry(context, DOCUMENT_IWA)?;
            let expanded = decode_iwa(&raw, context.budget().max_expanded_bytes as usize)?;
            context.record_expanded(expanded.len() as u64)?;
            self.document_objects = Some(parse_archive_objects(&expanded)?);
        }
        Ok(self
            .document_objects
            .as_deref()
            .expect("document objects initialized"))
    }

    fn all_objects(&mut self, context: &ProbeContext) -> Result<&[ArchiveObject]> {
        if self.all_objects.is_none() {
            let mut objects = self.document_objects(context)?.to_vec();
            let iwa_names = self
                .entry_names
                .iter()
                .filter(|name| name.to_ascii_lowercase().ends_with(".iwa"))
                .filter(|name| name.as_str() != DOCUMENT_IWA)
                .cloned()
                .collect::<Vec<_>>();
            for name in iwa_names {
                context.check_time()?;
                let raw = self.read_entry(context, &name)?;
                let expanded = decode_iwa(&raw, context.budget().max_expanded_bytes as usize)?;
                context.record_expanded(expanded.len() as u64)?;
                let remaining = MAX_IWA_OBJECTS.saturating_sub(objects.len());
                let decoded = parse_archive_objects(&expanded)?;
                if decoded.len() > remaining {
                    return Err(DeckProbeError::BudgetExceeded(format!(
                        "IWA object count exceeds {MAX_IWA_OBJECTS}"
                    )));
                }
                objects.extend(decoded);
            }
            self.all_objects = Some(objects);
        }
        Ok(self
            .all_objects
            .as_deref()
            .expect("all IWA objects initialized"))
    }
}

#[derive(Clone)]
struct IworkProperties {
    file_format_version: Option<String>,
    is_multi_page: Option<bool>,
    has_external_or_missing_data: Option<bool>,
    language: Option<String>,
    locale: Option<String>,
}

#[derive(Debug, Clone)]
struct ArchiveObject {
    identifier: u64,
    message_type: u64,
    payload: Vec<u8>,
}

#[derive(Debug)]
struct MessageInfo {
    message_type: u64,
    payload_length: usize,
}

fn decode_iwa(data: &[u8], expanded_limit: usize) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut decoder = SnappyDecoder::new();
    let mut offset = 0usize;
    while offset < data.len() {
        if data.len() - offset < 4 {
            return Err(DeckProbeError::MalformedInput(
                "trailing bytes after IWA Snappy chunk".to_owned(),
            ));
        }
        let chunk_type = data[offset];
        let length = usize::from(data[offset + 1])
            | (usize::from(data[offset + 2]) << 8)
            | (usize::from(data[offset + 3]) << 16);
        offset += 4;
        let end = offset.checked_add(length).ok_or_else(|| {
            DeckProbeError::MalformedInput("IWA chunk length overflow".to_owned())
        })?;
        if end > data.len() {
            return Err(DeckProbeError::MalformedInput(
                "IWA chunk exceeds ZIP entry".to_owned(),
            ));
        }
        let block = &data[offset..end];
        match chunk_type {
            0 => {
                let expected = decompress_len(block).map_err(|error| {
                    DeckProbeError::MalformedInput(format!(
                        "invalid IWA Snappy block header: {error}"
                    ))
                })?;
                ensure_expanded_limit(output.len(), expected, expanded_limit)?;
                let decoded = decoder.decompress_vec(block).map_err(|error| {
                    DeckProbeError::MalformedInput(format!("invalid IWA Snappy block: {error}"))
                })?;
                if decoded.len() != expected {
                    return Err(DeckProbeError::MalformedInput(
                        "IWA Snappy length mismatch".to_owned(),
                    ));
                }
                output.extend_from_slice(&decoded);
            }
            1 => {
                ensure_expanded_limit(output.len(), block.len(), expanded_limit)?;
                output.extend_from_slice(block);
            }
            other => {
                return Err(DeckProbeError::MalformedInput(format!(
                    "unsupported IWA chunk type {other:#04x}"
                )));
            }
        }
        offset = end;
    }
    Ok(output)
}

fn ensure_expanded_limit(current: usize, additional: usize, limit: usize) -> Result<()> {
    let next = current
        .checked_add(additional)
        .ok_or_else(|| DeckProbeError::BudgetExceeded("IWA expanded length overflow".to_owned()))?;
    if next > limit {
        return Err(DeckProbeError::BudgetExceeded(format!(
            "IWA expanded bytes {next} exceed per-entry limit {limit}"
        )));
    }
    Ok(())
}

fn parse_archive_objects(data: &[u8]) -> Result<Vec<ArchiveObject>> {
    let mut offset = 0usize;
    let mut objects = Vec::new();
    while offset < data.len() {
        if objects.len() >= MAX_IWA_OBJECTS {
            return Err(DeckProbeError::BudgetExceeded(format!(
                "IWA object count exceeds {MAX_IWA_OBJECTS}"
            )));
        }
        let (archive_length, prefix_length) = read_varint(&data[offset..])?;
        offset += prefix_length;
        let archive_length = usize::try_from(archive_length).map_err(|_| {
            DeckProbeError::MalformedInput("ArchiveInfo length overflow".to_owned())
        })?;
        let archive_end = offset.checked_add(archive_length).ok_or_else(|| {
            DeckProbeError::MalformedInput("ArchiveInfo length overflow".to_owned())
        })?;
        if archive_end > data.len() {
            return Err(DeckProbeError::MalformedInput(
                "ArchiveInfo exceeds IWA stream".to_owned(),
            ));
        }
        let (identifier, message_infos) = parse_archive_info(&data[offset..archive_end])?;
        offset = archive_end;
        for info in message_infos {
            let payload_end = offset.checked_add(info.payload_length).ok_or_else(|| {
                DeckProbeError::MalformedInput("IWA payload length overflow".to_owned())
            })?;
            if payload_end > data.len() {
                return Err(DeckProbeError::MalformedInput(
                    "IWA object payload exceeds stream".to_owned(),
                ));
            }
            objects.push(ArchiveObject {
                identifier,
                message_type: info.message_type,
                payload: data[offset..payload_end].to_vec(),
            });
            offset = payload_end;
        }
    }
    Ok(objects)
}

fn parse_archive_info(data: &[u8]) -> Result<(u64, Vec<MessageInfo>)> {
    let fields = parse_fields(data)?;
    let identifier = fields
        .iter()
        .find_map(|field| (field.number == 1).then(|| field.varint()).flatten())
        .ok_or_else(|| {
            DeckProbeError::MalformedInput("ArchiveInfo missing identifier".to_owned())
        })?;
    let mut infos = Vec::new();
    for field in fields.iter().filter(|field| field.number == 2) {
        let message = field.bytes().ok_or_else(|| {
            DeckProbeError::MalformedInput("ArchiveInfo MessageInfo has wrong wire type".to_owned())
        })?;
        let message_fields = parse_fields(message)?;
        let message_type = message_fields
            .iter()
            .find_map(|field| (field.number == 1).then(|| field.varint()).flatten())
            .ok_or_else(|| DeckProbeError::MalformedInput("MessageInfo missing type".to_owned()))?;
        let payload_length = message_fields
            .iter()
            .find_map(|field| (field.number == 3).then(|| field.varint()).flatten())
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| {
                DeckProbeError::MalformedInput("MessageInfo missing payload length".to_owned())
            })?;
        infos.push(MessageInfo {
            message_type,
            payload_length,
        });
    }
    if infos.is_empty() {
        return Err(DeckProbeError::MalformedInput(
            "ArchiveInfo contains no MessageInfo".to_owned(),
        ));
    }
    Ok((identifier, infos))
}

#[derive(Debug, Clone, Copy)]
struct ProtoField<'a> {
    number: u64,
    value: ProtoValue<'a>,
}

#[derive(Debug, Clone, Copy)]
enum ProtoValue<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> ProtoField<'a> {
    fn varint(self) -> Option<u64> {
        match self.value {
            ProtoValue::Varint(value) => Some(value),
            _ => None,
        }
    }

    fn bytes(self) -> Option<&'a [u8]> {
        match self.value {
            ProtoValue::Bytes(value) => Some(value),
            _ => None,
        }
    }

    fn fixed32(self) -> Option<u32> {
        match self.value {
            ProtoValue::Fixed32(value) => Some(value),
            _ => None,
        }
    }

    fn fixed64(self) -> Option<u64> {
        match self.value {
            ProtoValue::Fixed64(value) => Some(value),
            _ => None,
        }
    }

    fn float(self) -> Option<f32> {
        self.fixed32().map(f32::from_bits)
    }

    fn double(self) -> Option<f64> {
        self.fixed64().map(f64::from_bits)
    }
}

fn parse_fields(data: &[u8]) -> Result<Vec<ProtoField<'_>>> {
    let mut fields = Vec::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let (tag, tag_length) = read_varint(&data[offset..])?;
        offset += tag_length;
        let number = tag >> 3;
        let wire = tag & 7;
        if number == 0 {
            return Err(DeckProbeError::MalformedInput(
                "protobuf field number zero".to_owned(),
            ));
        }
        let value = match wire {
            0 => {
                let (value, length) = read_varint(&data[offset..])?;
                offset += length;
                ProtoValue::Varint(value)
            }
            1 => {
                let end = checked_field_end(data, offset, 8)?;
                let value = u64::from_le_bytes(
                    data[offset..end]
                        .try_into()
                        .expect("fixed64 field has eight bytes"),
                );
                offset = end;
                ProtoValue::Fixed64(value)
            }
            2 => {
                let (length, prefix) = read_varint(&data[offset..])?;
                offset += prefix;
                let length = usize::try_from(length).map_err(|_| {
                    DeckProbeError::MalformedInput("protobuf field length overflow".to_owned())
                })?;
                let end = checked_field_end(data, offset, length)?;
                let value = ProtoValue::Bytes(&data[offset..end]);
                offset = end;
                value
            }
            5 => {
                let end = checked_field_end(data, offset, 4)?;
                let value = u32::from_le_bytes(
                    data[offset..end]
                        .try_into()
                        .expect("fixed32 field has four bytes"),
                );
                offset = end;
                ProtoValue::Fixed32(value)
            }
            _ => {
                return Err(DeckProbeError::MalformedInput(format!(
                    "unsupported protobuf wire type {wire}"
                )));
            }
        };
        fields.push(ProtoField { number, value });
    }
    Ok(fields)
}

fn checked_field_end(data: &[u8], offset: usize, length: usize) -> Result<usize> {
    offset
        .checked_add(length)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| DeckProbeError::MalformedInput("protobuf field exceeds message".to_owned()))
}

fn read_varint(data: &[u8]) -> Result<(u64, usize)> {
    let mut value = 0u64;
    for (index, byte) in data.iter().copied().take(10).enumerate() {
        if index == 9 && byte > 1 {
            break;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok((value, index + 1));
        }
    }
    Err(DeckProbeError::MalformedInput(
        "invalid or truncated protobuf varint".to_owned(),
    ))
}

fn numbers_sheets(objects: &[ArchiveObject]) -> Option<Vec<String>> {
    let sheet_ids = objects
        .iter()
        .filter(|object| object.message_type == 1)
        .find_map(|object| {
            let ids = parse_fields(&object.payload)
                .ok()?
                .into_iter()
                .filter(|field| field.number == 1)
                .filter_map(|field| field.bytes())
                .filter_map(parse_reference)
                .collect::<Vec<_>>();
            (!ids.is_empty()).then_some(ids)
        })?;
    let names = objects
        .iter()
        .filter(|object| object.message_type == 2)
        .filter_map(|object| sheet_name(&object.payload).map(|name| (object.identifier, name)))
        .collect::<BTreeMap<_, _>>();
    sheet_ids
        .into_iter()
        .map(|identifier| names.get(&identifier).cloned())
        .collect()
}

fn parse_reference(data: &[u8]) -> Option<u64> {
    parse_fields(data)
        .ok()?
        .into_iter()
        .find_map(|field| (field.number == 1).then(|| field.varint()).flatten())
}

fn sheet_name(data: &[u8]) -> Option<String> {
    parse_fields(data)
        .ok()?
        .into_iter()
        .find_map(|field| (field.number == 1).then(|| field.bytes()).flatten())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .filter(|name| !name.chars().any(char::is_control))
        .map(str::to_owned)
}

fn common_deep_evidence(objects: &[ArchiveObject], path: &str) -> Vec<Evidence> {
    let mut message_type_counts = BTreeMap::<String, u64>::new();
    for object in objects {
        *message_type_counts
            .entry(object.message_type.to_string())
            .or_default() += 1;
    }
    let object_type_counts = BTreeMap::from([
        (
            "chart",
            count_message_types(objects, &[TSCH_CHART_DRAWABLE_ARCHIVE]),
        ),
        (
            "comment_info",
            count_message_types(objects, &[TSWP_COMMENT_INFO_ARCHIVE]),
        ),
        (
            "comment_storage",
            count_message_types(objects, &[TSD_COMMENT_STORAGE_ARCHIVE]),
        ),
        ("image", count_message_types(objects, &[TSD_IMAGE_ARCHIVE])),
        ("movie", count_message_types(objects, &[TSD_MOVIE_ARCHIVE])),
        (
            "table_info",
            count_message_types(
                objects,
                &[TST_TABLE_INFO_ARCHIVE, TST_WP_TABLE_INFO_ARCHIVE],
            ),
        ),
        (
            "table_model",
            count_message_types(objects, &[TST_TABLE_MODEL_ARCHIVE]),
        ),
        (
            "text_storage",
            count_message_types(objects, &TSWP_STORAGE_ARCHIVE),
        ),
    ]);
    vec![
        Evidence::resolved(
            "iwork.archive_object_count",
            json!(objects.len()),
            Confidence::Exact,
            path,
            "all decoded IWA ArchiveInfo message frames",
        ),
        Evidence::resolved(
            "iwork.message_type_counts",
            json!(message_type_counts),
            Confidence::Exact,
            path,
            "IWA MessageInfo type identifiers",
        ),
        Evidence::resolved(
            "iwork.object_type_counts",
            json!(object_type_counts),
            Confidence::Exact,
            path,
            "stable iWork Protobuf message-type mapping",
        ),
    ]
}

fn integrity_result_evidence(path: &str) -> Vec<Evidence> {
    vec![
        Evidence::resolved(
            "iwork.all_iwa_valid",
            true,
            Confidence::Exact,
            path,
            "all IWA Snappy streams and protobuf archive frames",
        ),
        Evidence::resolved(
            "quality.corrupted",
            false,
            Confidence::Exact,
            path,
            "complete IWA package validation",
        ),
    ]
}

fn keynote_deep_evidence(objects: &[ArchiveObject], path: &str) -> Vec<Evidence> {
    let slide_nodes = objects
        .iter()
        .filter(|object| object.message_type == 4)
        .filter_map(|object| {
            let fields = parse_fields(&object.payload).ok()?;
            fields
                .iter()
                .copied()
                .any(|field| field.number == 2 && field.bytes().and_then(parse_reference).is_some())
                .then_some(fields)
        })
        .collect::<Vec<_>>();
    let count_flag = |number| {
        slide_nodes
            .iter()
            .filter(|fields| proto_bool(fields, number).unwrap_or(false))
            .count()
    };
    let size = objects
        .iter()
        .find(|object| object.message_type == 2)
        .and_then(|object| proto_bytes(&object.payload, 4))
        .and_then(proto_size);
    let table_count = logical_table_metrics(objects).len();

    let mut evidence = canvas_evidence(
        "keynote.slide_size",
        "keynote.aspect_ratio",
        "keynote.orientation",
        size,
        path,
        "KN.ShowArchive size",
    );
    evidence.extend([
        schema_count_evidence(
            "keynote.hidden_slide_count",
            count_flag(4),
            path,
            "KN.SlideNodeArchive isHidden",
        ),
        schema_count_evidence(
            "keynote.slides_with_builds_count",
            count_flag(6),
            path,
            "KN.SlideNodeArchive hasBuilds",
        ),
        schema_count_evidence(
            "keynote.slides_with_transitions_count",
            count_flag(7),
            path,
            "KN.SlideNodeArchive hasTransition",
        ),
        schema_count_evidence(
            "keynote.slides_with_notes_count",
            count_flag(8),
            path,
            "KN.SlideNodeArchive hasNote",
        ),
        schema_count_evidence(
            "keynote.table_count",
            table_count,
            path,
            "TST.TableInfoArchive tableModel references",
        ),
    ]);
    evidence
}

fn numbers_deep_evidence(objects: &[ArchiveObject], path: &str) -> Vec<Evidence> {
    let tables = logical_table_metrics(objects);
    let hidden_rows = tables.iter().map(|table| table.hidden_rows).sum::<u64>();
    let hidden_columns = tables.iter().map(|table| table.hidden_columns).sum::<u64>();
    let filtered_rows = tables.iter().map(|table| table.filtered_rows).sum::<u64>();
    let dimensions = tables
        .iter()
        .map(|table| {
            json!({
                "name": table.name,
                "rows": table.rows,
                "columns": table.columns,
                "hidden_rows": table.hidden_rows,
                "hidden_columns": table.hidden_columns,
                "filtered_rows": table.filtered_rows,
                "default_row_height_pt": table.default_row_height,
                "default_column_width_pt": table.default_column_width,
            })
        })
        .collect::<Vec<_>>();
    let formula_definitions = objects
        .iter()
        .filter(|object| TST_TABLE_DATA_LIST_ARCHIVE.contains(&object.message_type))
        .filter_map(|object| parse_fields(&object.payload).ok())
        .filter(|fields| proto_varint_from_fields(fields, 1) == Some(3))
        .map(|fields| {
            fields
                .iter()
                .filter(|field| field.number == 3 && field.bytes().is_some())
                .count()
        })
        .sum::<usize>();

    vec![
        schema_count_evidence(
            "numbers.table_count",
            tables.len(),
            path,
            "TST.TableInfoArchive tableModel references",
        ),
        Evidence::resolved(
            "numbers.table_dimensions",
            json!(dimensions),
            Confidence::Exact,
            path,
            "TST.TableModelArchive dimensions and visibility counters",
        ),
        schema_count_evidence(
            "numbers.hidden_row_count",
            hidden_rows,
            path,
            "TST.TableModelArchive number_of_hidden_rows",
        ),
        schema_count_evidence(
            "numbers.hidden_column_count",
            hidden_columns,
            path,
            "TST.TableModelArchive number_of_hidden_columns",
        ),
        schema_count_evidence(
            "numbers.filtered_row_count",
            filtered_rows,
            path,
            "TST.TableModelArchive number_of_filtered_rows",
        ),
        schema_count_evidence(
            "numbers.formula_definition_count",
            formula_definitions,
            path,
            "TST.TableDataList FORMULA entries",
        ),
    ]
}

fn pages_deep_evidence(objects: &[ArchiveObject], path: &str) -> Vec<Evidence> {
    let document = objects.iter().find(|object| object.message_type == 10_000);
    let page_size = document.and_then(|object| {
        let fields = parse_fields(&object.payload).ok()?;
        Some((
            proto_float_from_fields(&fields, 30)?,
            proto_float_from_fields(&fields, 31)?,
        ))
    });
    let change_tracking = document
        .and_then(|object| proto_varint(&object.payload, 40))
        .is_some_and(|value| value != 0);
    let body_storage = document
        .and_then(|object| proto_bytes(&object.payload, 4))
        .and_then(parse_reference)
        .and_then(|identifier| {
            objects.iter().find(|object| {
                object.identifier == identifier
                    && TSWP_STORAGE_ARCHIVE.contains(&object.message_type)
            })
        });
    let body_text = body_storage.and_then(|object| storage_text(&object.payload));
    let body_length = body_text
        .as_ref()
        .map(|text| text.encode_utf16().count() as u64);
    let paragraph_breaks = body_text
        .as_ref()
        .map(|text| text.chars().filter(|character| *character == '\n').count() as u64);
    let sections = objects
        .iter()
        .filter(|object| object.message_type == 10_011)
        .collect::<Vec<_>>();
    let section_names = sections
        .iter()
        .filter_map(|object| proto_string(&object.payload, 26))
        .collect::<Vec<_>>();
    let cached_page_count = objects
        .iter()
        .find(|object| object.message_type == 10_131)
        .and_then(|object| proto_varint(&object.payload, 4));
    let table_count = logical_table_metrics(objects).len();

    let mut evidence = vec![
        schema_count_evidence(
            "pages.section_count",
            sections.len(),
            path,
            "TP.SectionArchive objects",
        ),
        Evidence::resolved(
            "pages.section_names",
            json!(section_names),
            Confidence::Exact,
            path,
            "TP.SectionArchive names",
        ),
        Evidence::resolved(
            "pages.change_tracking_enabled",
            change_tracking,
            Confidence::Exact,
            path,
            "TP.DocumentArchive change_tracking_enabled",
        ),
        Evidence::resolved(
            "pages.body_text_length",
            json!(body_length),
            Confidence::Exact,
            path,
            "TP.DocumentArchive body_storage -> TSWP.StorageArchive text",
        ),
        Evidence::resolved(
            "pages.body_paragraph_break_count",
            json!(paragraph_breaks),
            Confidence::Exact,
            path,
            "TSWP.StorageArchive body text newline characters",
        ),
        Evidence::resolved(
            "pages.cached_page_count",
            json!(cached_page_count),
            Confidence::High,
            path,
            "TP.LayoutStateArchive last_page_count cache",
        ),
        schema_count_evidence(
            "pages.table_count",
            table_count,
            path,
            "TST.TableInfoArchive tableModel references",
        ),
    ];
    evidence.extend(canvas_evidence(
        "pages.page_size",
        "pages.aspect_ratio",
        "pages.orientation",
        page_size,
        path,
        "TP.DocumentArchive page_width/page_height",
    ));
    evidence
}

#[derive(Debug)]
struct TableMetrics {
    identifier: u64,
    name: String,
    rows: u64,
    columns: u64,
    hidden_rows: u64,
    hidden_columns: u64,
    filtered_rows: u64,
    default_row_height: Option<f64>,
    default_column_width: Option<f64>,
}

fn logical_table_metrics(objects: &[ArchiveObject]) -> Vec<TableMetrics> {
    let model_ids = objects
        .iter()
        .filter_map(|object| match object.message_type {
            TST_TABLE_INFO_ARCHIVE => proto_bytes(&object.payload, 2).and_then(parse_reference),
            TST_WP_TABLE_INFO_ARCHIVE => proto_bytes(&object.payload, 1)
                .and_then(|table_info| proto_bytes(table_info, 2))
                .and_then(parse_reference),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut tables = objects
        .iter()
        .filter(|object| {
            object.message_type == TST_TABLE_MODEL_ARCHIVE && model_ids.contains(&object.identifier)
        })
        .filter_map(table_metrics)
        .collect::<Vec<_>>();
    tables.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.identifier.cmp(&right.identifier))
    });
    tables
}

fn table_metrics(object: &ArchiveObject) -> Option<TableMetrics> {
    let fields = parse_fields(&object.payload).ok()?;
    Some(TableMetrics {
        identifier: object.identifier,
        name: proto_string_from_fields(&fields, 8)
            .or_else(|| proto_string_from_fields(&fields, 1))
            .unwrap_or_default(),
        rows: proto_varint_from_fields(&fields, 6)?,
        columns: proto_varint_from_fields(&fields, 7)?,
        hidden_rows: proto_varint_from_fields(&fields, 14).unwrap_or(0),
        hidden_columns: proto_varint_from_fields(&fields, 15).unwrap_or(0),
        filtered_rows: proto_varint_from_fields(&fields, 40).unwrap_or(0),
        default_row_height: proto_double_from_fields(&fields, 16),
        default_column_width: proto_double_from_fields(&fields, 17),
    })
}

fn canvas_evidence(
    size_target: &str,
    ratio_target: &str,
    orientation_target: &str,
    size: Option<(f32, f32)>,
    path: &str,
    source: &str,
) -> Vec<Evidence> {
    match size.filter(|(width, height)| {
        width.is_finite() && height.is_finite() && *width > 0.0 && *height > 0.0
    }) {
        Some((width, height)) => {
            let orientation = match width.total_cmp(&height) {
                std::cmp::Ordering::Greater => "landscape",
                std::cmp::Ordering::Less => "portrait",
                std::cmp::Ordering::Equal => "square",
            };
            vec![
                Evidence::resolved(
                    size_target,
                    json!({"width_pt": width, "height_pt": height}),
                    Confidence::Exact,
                    path,
                    source,
                ),
                Evidence::resolved(
                    ratio_target,
                    json!({
                        "width": width,
                        "height": height,
                        "decimal": f64::from(width) / f64::from(height),
                    }),
                    Confidence::Exact,
                    path,
                    source,
                ),
                Evidence::resolved(
                    orientation_target,
                    orientation,
                    Confidence::Exact,
                    path,
                    source,
                ),
            ]
        }
        None => [size_target, ratio_target, orientation_target]
            .into_iter()
            .map(|target| {
                Evidence::resolved(
                    target,
                    serde_json::Value::Null,
                    Confidence::Exact,
                    path,
                    source,
                )
            })
            .collect(),
    }
}

fn count_message_types(objects: &[ArchiveObject], message_types: &[u64]) -> u64 {
    objects
        .iter()
        .filter(|object| message_types.contains(&object.message_type))
        .count() as u64
}

fn schema_count_evidence(
    target: &str,
    count: impl TryInto<u64>,
    path: &str,
    source: &str,
) -> Evidence {
    Evidence::resolved(
        target,
        json!(count.try_into().ok().unwrap_or(u64::MAX)),
        Confidence::Exact,
        path,
        source,
    )
}

fn proto_varint(data: &[u8], number: u64) -> Option<u64> {
    let fields = parse_fields(data).ok()?;
    proto_varint_from_fields(&fields, number)
}

fn proto_varint_from_fields(fields: &[ProtoField<'_>], number: u64) -> Option<u64> {
    fields
        .iter()
        .copied()
        .find_map(|field| (field.number == number).then(|| field.varint()).flatten())
}

fn proto_bool(fields: &[ProtoField<'_>], number: u64) -> Option<bool> {
    proto_varint_from_fields(fields, number).map(|value| value != 0)
}

fn proto_bytes(data: &[u8], number: u64) -> Option<&[u8]> {
    parse_fields(data)
        .ok()?
        .into_iter()
        .find_map(|field| (field.number == number).then(|| field.bytes()).flatten())
}

fn proto_string(data: &[u8], number: u64) -> Option<String> {
    parse_fields(data)
        .ok()
        .and_then(|fields| proto_string_from_fields(&fields, number))
}

fn proto_string_from_fields(fields: &[ProtoField<'_>], number: u64) -> Option<String> {
    fields
        .iter()
        .copied()
        .find_map(|field| (field.number == number).then(|| field.bytes()).flatten())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .filter(|value| !value.chars().any(char::is_control))
        .map(str::to_owned)
}

fn proto_float_from_fields(fields: &[ProtoField<'_>], number: u64) -> Option<f32> {
    fields
        .iter()
        .copied()
        .find_map(|field| (field.number == number).then(|| field.float()).flatten())
}

fn proto_double_from_fields(fields: &[ProtoField<'_>], number: u64) -> Option<f64> {
    fields
        .iter()
        .copied()
        .find_map(|field| (field.number == number).then(|| field.double()).flatten())
}

fn proto_size(data: &[u8]) -> Option<(f32, f32)> {
    let fields = parse_fields(data).ok()?;
    Some((
        proto_float_from_fields(&fields, 1)?,
        proto_float_from_fields(&fields, 2)?,
    ))
}

fn storage_text(data: &[u8]) -> Option<String> {
    let fields = parse_fields(data).ok()?;
    let segments = fields
        .iter()
        .copied()
        .filter(|field| field.number == 3)
        .map(|field| std::str::from_utf8(field.bytes()?).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(segments.concat())
}

fn validate_generation(names: &BTreeSet<&str>) -> Result<()> {
    if names.contains(DOCUMENT_IWA) {
        return Ok(());
    }
    if names.contains("index.apxl") || names.contains("index.xml") {
        return Err(DeckProbeError::UnsupportedFormat(
            "legacy XML iWork packages are not supported; modern IWA is required".to_owned(),
        ));
    }
    Err(DeckProbeError::MalformedInput(format!(
        "iWork ZIP package is missing {DOCUMENT_IWA}"
    )))
}

fn check_entry_budget(context: &ProbeContext, count: usize) -> Result<()> {
    if count > context.budget().max_archive_entries {
        return Err(DeckProbeError::BudgetExceeded(format!(
            "archive entries {count} exceed budget {}",
            context.budget().max_archive_entries
        )));
    }
    Ok(())
}

fn map_zip_error(
    context: &ProbeContext,
    prefix: &str,
    error: zip::result::ZipError,
) -> DeckProbeError {
    if let Some(message) = context.budget_failure() {
        DeckProbeError::BudgetExceeded(message)
    } else if context.check_time().is_err() || error.to_string().contains("deckprobe") {
        DeckProbeError::BudgetExceeded(
            context
                .budget_failure()
                .unwrap_or_else(|| error.to_string()),
        )
    } else {
        DeckProbeError::MalformedInput(format!("{prefix}: {error}"))
    }
}

fn is_component(name: &str, prefix: &str) -> bool {
    name == format!("{prefix}.iwa")
        || name.strip_prefix(prefix).is_some_and(|suffix| {
            !suffix.contains('/')
                && suffix.ends_with(".iwa")
                && (suffix.starts_with('-')
                    || suffix.as_bytes().first().is_some_and(u8::is_ascii_digit))
        })
}

fn is_preview_entry(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "preview.jpg"
        || name == "preview.jpeg"
        || name == "preview.png"
        || name.starts_with("preview-")
}

fn asset_type_counts(names: &[String]) -> BTreeMap<&'static str, u64> {
    let mut counts = BTreeMap::from([
        ("audio", 0),
        ("font", 0),
        ("image", 0),
        ("other", 0),
        ("video", 0),
    ]);
    for name in names
        .iter()
        .filter(|name| name.starts_with("Data/") && !name.ends_with('/'))
    {
        let extension = name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        let kind = match extension.as_deref() {
            Some(
                "bmp" | "gif" | "heic" | "heif" | "jpeg" | "jpg" | "png" | "svg" | "tif" | "tiff"
                | "webp",
            ) => "image",
            Some("aac" | "aif" | "aiff" | "caf" | "m4a" | "mp3" | "wav") => "audio",
            Some("avi" | "m4v" | "mov" | "mp4" | "mpeg" | "mpg") => "video",
            Some("otf" | "ttf" | "woff" | "woff2") => "font",
            _ => "other",
        };
        *counts.get_mut(kind).expect("asset kind initialized") += 1;
    }
    counts
}

fn latest_producer_build(value: &PlistValue) -> Option<String> {
    value.as_array()?.iter().rev().find_map(|entry| {
        entry
            .as_string()
            .filter(|value| !value.starts_with("Template:"))
            .map(str::to_owned)
    })
}

fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") && data.len() >= 24 {
        let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
        return Some((width, height));
    }
    jpeg_dimensions(data)
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if !data.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut offset = 2usize;
    while offset + 3 < data.len() {
        while offset < data.len() && data[offset] != 0xff {
            offset += 1;
        }
        while offset < data.len() && data[offset] == 0xff {
            offset += 1;
        }
        let marker = *data.get(offset)?;
        offset += 1;
        if matches!(marker, 0x01 | 0xd0..=0xd9) {
            continue;
        }
        let segment_length = usize::from(u16::from_be_bytes([
            *data.get(offset)?,
            *data.get(offset + 1)?,
        ]));
        if segment_length < 2 || offset.checked_add(segment_length)? > data.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) && segment_length >= 7
        {
            let height = u32::from(u16::from_be_bytes([data[offset + 3], data[offset + 4]]));
            let width = u32::from(u16::from_be_bytes([data[offset + 5], data[offset + 6]]));
            return Some((width, height));
        }
        offset += segment_length;
    }
    None
}

fn count_components(names: &[String], prefix: &str) -> usize {
    names
        .iter()
        .filter(|name| is_component(name, prefix))
        .count()
}

fn count_evidence(target: &str, count: usize, path: &str) -> Evidence {
    Evidence::resolved(
        target,
        json!(count),
        Confidence::Exact,
        path,
        "IWA component inventory",
    )
}

fn application_name(kind: IworkKind) -> &'static str {
    match kind {
        IworkKind::Keynote => "Apple Keynote",
        IworkKind::Numbers => "Apple Numbers",
        IworkKind::Pages => "Apple Pages",
    }
}

fn optional_string_evidence(
    target: &str,
    value: Option<&str>,
    path: &str,
    source: &str,
) -> Evidence {
    match value {
        Some(value) => Evidence::resolved(target, value, Confidence::High, path, source),
        None => Evidence::resolved(
            target,
            serde_json::Value::Null,
            Confidence::High,
            path,
            source,
        ),
    }
}

fn optional_bool_evidence(target: &str, value: Option<bool>, path: &str, source: &str) -> Evidence {
    match value {
        Some(value) => Evidence::resolved(target, value, Confidence::High, path, source),
        None => Evidence::resolved(
            target,
            serde_json::Value::Null,
            Confidence::High,
            path,
            source,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snap::raw::Encoder as SnappyEncoder;

    #[test]
    fn decodes_compressed_and_stored_iwa_chunks() {
        let compressed = SnappyEncoder::new().compress_vec(b"first").unwrap();
        let mut input = chunk(0, &compressed);
        input.extend(chunk(1, b"-second"));
        assert_eq!(decode_iwa(&input, 64).unwrap(), b"first-second");
    }

    #[test]
    fn rejects_unknown_or_truncated_iwa_chunks() {
        assert!(decode_iwa(&chunk(9, b"bad"), 64).is_err());
        assert!(decode_iwa(&[0, 4, 0, 0, 1], 64).is_err());
    }

    #[test]
    fn parses_archive_info_and_payload_frames() {
        let payload = field_bytes(1, b"Sheet 1");
        let mut message_info = field_varint(1, 2);
        message_info.extend(field_varint(3, payload.len() as u64));
        let mut archive_info = field_varint(1, 42);
        archive_info.extend(field_bytes(2, &message_info));
        let mut stream = varint(archive_info.len() as u64);
        stream.extend(archive_info);
        stream.extend(payload);

        let objects = parse_archive_objects(&stream).unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].identifier, 42);
        assert_eq!(objects[0].message_type, 2);
        assert_eq!(sheet_name(&objects[0].payload).as_deref(), Some("Sheet 1"));
    }

    #[test]
    fn generation_check_rejects_legacy_xml() {
        let names = BTreeSet::from(["index.apxl"]);
        let error = validate_generation(&names).unwrap_err();
        assert!(matches!(error, DeckProbeError::UnsupportedFormat(_)));
        assert!(error.to_string().contains("modern IWA is required"));
    }

    #[test]
    fn extracts_latest_non_template_producer_build() {
        let history = PlistValue::Array(vec![
            PlistValue::String("Template: Blank (13.2)".to_owned()),
            PlistValue::String("M14.1-7040.0.73-4".to_owned()),
            PlistValue::String("M14.4-7043.0.93-4".to_owned()),
        ]);
        assert_eq!(
            latest_producer_build(&history).as_deref(),
            Some("M14.4-7043.0.93-4")
        );
    }

    #[test]
    fn classifies_data_assets_and_reads_preview_dimensions() {
        let names = vec![
            "Data/photo.JPG".to_owned(),
            "Data/audio.m4a".to_owned(),
            "Data/movie.mov".to_owned(),
            "Data/font.otf".to_owned(),
            "Data/blob.bin".to_owned(),
            "Index/Document.iwa".to_owned(),
        ];
        let counts = asset_type_counts(&names);
        assert_eq!(counts["image"], 1);
        assert_eq!(counts["audio"], 1);
        assert_eq!(counts["video"], 1);
        assert_eq!(counts["font"], 1);
        assert_eq!(counts["other"], 1);

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(image_dimensions(&png), Some((640, 480)));

        let jpeg = [
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x07, 0x08, 0x01, 0xe0, 0x02, 0x80,
        ];
        assert_eq!(image_dimensions(&jpeg), Some((640, 480)));
    }

    #[test]
    fn decodes_stable_keynote_numbers_and_pages_schema_fields() {
        let keynote_size = [field_fixed32(1, 1920.0), field_fixed32(2, 1080.0)].concat();
        let keynote_show = field_bytes(4, &keynote_size);
        let keynote_slide = [
            field_bytes(2, &field_varint(1, 100)),
            field_varint(4, 1),
            field_varint(5, 0),
            field_varint(6, 1),
            field_varint(7, 1),
            field_varint(8, 1),
        ]
        .concat();
        let keynote = vec![object(1, 2, keynote_show), object(2, 4, keynote_slide)];
        let evidence = keynote_deep_evidence(&keynote, "deep");
        assert_eq!(
            evidence_value(&evidence, "keynote.orientation"),
            json!("landscape")
        );
        assert_eq!(
            evidence_value(&evidence, "keynote.hidden_slide_count"),
            json!(1)
        );
        assert_eq!(
            evidence_value(&evidence, "keynote.slides_with_notes_count"),
            json!(1)
        );

        let table_info = field_bytes(2, &field_varint(1, 20));
        let table_model = [
            field_bytes(1, b"table-id"),
            field_varint(6, 12),
            field_varint(7, 7),
            field_bytes(8, b"Table 1"),
            field_varint(14, 2),
            field_varint(15, 1),
            field_fixed64(16, 20.0),
            field_fixed64(17, 80.0),
            field_varint(40, 3),
        ]
        .concat();
        let formula_list = [
            field_varint(1, 3),
            field_bytes(3, &field_varint(1, 1)),
            field_bytes(3, &field_varint(1, 2)),
        ]
        .concat();
        let numbers = vec![
            object(10, TST_TABLE_INFO_ARCHIVE, table_info),
            object(20, TST_TABLE_MODEL_ARCHIVE, table_model),
            object(30, TST_TABLE_DATA_LIST_ARCHIVE[0], formula_list),
        ];
        let evidence = numbers_deep_evidence(&numbers, "deep");
        assert_eq!(evidence_value(&evidence, "numbers.table_count"), json!(1));
        assert_eq!(
            evidence_value(&evidence, "numbers.formula_definition_count"),
            json!(2)
        );
        assert_eq!(
            evidence_value(&evidence, "numbers.filtered_row_count"),
            json!(3)
        );

        let pages_document = [
            field_bytes(4, &field_varint(1, 50)),
            field_fixed32(30, 595.28),
            field_fixed32(31, 841.89),
            field_varint(40, 1),
        ]
        .concat();
        let pages_storage = field_bytes(3, "one\ntwo😀".as_bytes());
        let pages = vec![
            object(40, 10_000, pages_document),
            object(50, TSWP_STORAGE_ARCHIVE[0], pages_storage),
            object(60, 10_011, field_bytes(26, b"Section")),
            object(70, 10_131, field_varint(4, 2)),
        ];
        let evidence = pages_deep_evidence(&pages, "deep");
        assert_eq!(evidence_value(&evidence, "pages.section_count"), json!(1));
        assert_eq!(
            evidence_value(&evidence, "pages.body_text_length"),
            json!(9)
        );
        assert_eq!(
            evidence_value(&evidence, "pages.body_paragraph_break_count"),
            json!(1)
        );
        assert_eq!(
            evidence_value(&evidence, "pages.change_tracking_enabled"),
            json!(true)
        );
    }

    fn chunk(kind: u8, payload: &[u8]) -> Vec<u8> {
        let length = payload.len();
        let mut output = vec![
            kind,
            (length & 0xff) as u8,
            ((length >> 8) & 0xff) as u8,
            ((length >> 16) & 0xff) as u8,
        ];
        output.extend_from_slice(payload);
        output
    }

    fn field_varint(number: u64, value: u64) -> Vec<u8> {
        let mut output = varint(number << 3);
        output.extend(varint(value));
        output
    }

    fn field_bytes(number: u64, value: &[u8]) -> Vec<u8> {
        let mut output = varint((number << 3) | 2);
        output.extend(varint(value.len() as u64));
        output.extend_from_slice(value);
        output
    }

    fn field_fixed32(number: u64, value: f32) -> Vec<u8> {
        let mut output = varint((number << 3) | 5);
        output.extend(value.to_bits().to_le_bytes());
        output
    }

    fn field_fixed64(number: u64, value: f64) -> Vec<u8> {
        let mut output = varint((number << 3) | 1);
        output.extend(value.to_bits().to_le_bytes());
        output
    }

    fn object(identifier: u64, message_type: u64, payload: Vec<u8>) -> ArchiveObject {
        ArchiveObject {
            identifier,
            message_type,
            payload,
        }
    }

    fn evidence_value(evidence: &[Evidence], target: &str) -> serde_json::Value {
        evidence
            .iter()
            .find(|item| item.target == target)
            .and_then(|item| item.value.clone())
            .unwrap_or_else(|| panic!("missing value for {target}"))
    }

    fn varint(mut value: u64) -> Vec<u8> {
        let mut output = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            output.push(byte);
            if value == 0 {
                return output;
            }
        }
    }
}
