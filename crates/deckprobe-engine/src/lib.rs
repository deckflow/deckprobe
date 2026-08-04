use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use deckprobe_core::{
    Budget, Confidence, DeckProbeError, Diagnostic, DriverReport, Evidence, ExecutionReport,
    FormatDriver, InputReport, MemorySource, ProbeContext, ProbeLevel, ProbeOptions, ProbeReport,
    ProbeRequest, ProbeSource, Result, TargetScope, TargetSpec, TargetStatus, plan_paths,
};
use deckprobe_format_excel::ExcelDriver;
use deckprobe_format_iwork::{IworkDriver, IworkKind};
use deckprobe_format_office_legacy::{OfficeLegacyDriver, profile_for_extension};
use deckprobe_format_pdf::PdfDriver;
use deckprobe_format_powerpoint::PowerPointDriver;
use deckprobe_format_word::WordDriver;
use serde::Serialize;
use serde_json::{Value, json};

pub const SCHEMA_VERSION: u32 = 2;
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Probe any re-openable source through the shared driver engine.
pub fn probe(source: Arc<dyn ProbeSource>, options: ProbeOptions) -> Result<ProbeReport> {
    let mut budget = Budget::for_level(options.level);
    options.budget.apply_to(&mut budget);
    let mut context = ProbeContext::new(source, budget)?;
    let driver = detect_driver(&mut context)?;

    if let Some(forced) = &options.input_format {
        let normalized = forced.to_ascii_lowercase();
        if !forced_format_matches(&normalized, &*driver) {
            return Err(DeckProbeError::InvalidRequest(format!(
                "forced format {forced} does not match detected {}/{}",
                driver.id(),
                driver.profile().profile
            )));
        }
    }

    let target_specs = driver.targets();
    let targets = expand_targets(&options.targets, &*driver, &target_specs, options.level)?;
    let mut optional_targets = if options.optional_targets.is_empty() {
        BTreeSet::new()
    } else {
        expand_targets(
            &options.optional_targets,
            &*driver,
            &target_specs,
            options.level,
        )?
    };
    optional_targets.retain(|target| !targets.contains(target));
    let all_requested = targets
        .union(&optional_targets)
        .cloned()
        .collect::<BTreeSet<_>>();
    let target_confidence =
        resolve_target_confidence(&options.target_confidence, &target_specs, &all_requested)?;
    let request = ProbeRequest {
        targets,
        optional_targets,
        level: options.level,
        minimum_confidence: options.minimum_confidence,
        target_confidence,
        allow_piggyback: options.allow_piggyback,
        format_options: resolve_format_options(&options.format_options, &*driver)?,
    };
    driver.validate_options(&request)?;
    let paths = driver.paths(&request)?;
    let plan = plan_paths(&request, &paths)?;

    let evidence = if options.plan_only {
        Vec::new()
    } else {
        driver.execute(&mut context, &request, &plan)?
    };
    let displayed_targets = request
        .targets
        .iter()
        .chain(plan.piggyback_targets.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let (results, diagnostics, unresolved_targets) = merge_results(
        &displayed_targets,
        &plan.unresolved_targets,
        evidence,
        options.plan_only,
        &request,
    );
    let status = if !options.plan_only && !unresolved_targets.is_empty() {
        "partial"
    } else {
        "ok"
    };

    Ok(ProbeReport {
        schema_version: SCHEMA_VERSION,
        tool_version: TOOL_VERSION.to_owned(),
        status: status.to_owned(),
        input: InputReport {
            display_name: context.display_name().to_owned(),
            source_kind: context.source_kind().to_owned(),
            file_size: context.file_size(),
        },
        driver: DriverReport {
            id: driver.id().to_owned(),
            profile: driver.profile().profile.to_owned(),
        },
        results,
        execution: ExecutionReport {
            probe_level: options.level,
            paths: plan.paths,
            estimated_cost: plan.estimated_cost,
            actual_cost: context.cost_snapshot(options.telemetry),
            unresolved_targets,
            piggyback_targets: plan.piggyback_targets,
        },
        diagnostics,
    })
}

pub fn probe_source<S>(source: S, options: ProbeOptions) -> Result<ProbeReport>
where
    S: ProbeSource + 'static,
{
    probe(Arc::new(source), options)
}

pub fn probe_bytes(
    display_name: impl Into<String>,
    bytes: impl Into<Arc<[u8]>>,
    options: ProbeOptions,
) -> Result<ProbeReport> {
    probe_source(MemorySource::new(display_name, bytes), options)
}

pub fn values_report(report: &ProbeReport) -> Value {
    let values = report
        .results
        .iter()
        .filter_map(|(target, evidence)| {
            evidence
                .value
                .as_ref()
                .map(|value| (target.clone(), value.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    json!({
        "schema_version": report.schema_version,
        "tool_version": report.tool_version,
        "status": report.status,
        "view": "values",
        "input": report.input,
        "driver": report.driver,
        "values": values,
        "unresolved_targets": report.execution.unresolved_targets,
        "piggyback_targets": report.execution.piggyback_targets,
        "diagnostics": report.diagnostics,
    })
}

pub fn formats_report() -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "tool_version": TOOL_VERSION,
        "status": "ok",
        "formats": [
            {"driver": "pdf", "profiles": ["pdf"], "support": "2.0"},
            {"driver": "word", "profiles": ["docx", "docm", "dotx", "dotm"], "support": "2.0"},
            {"driver": "excel", "profiles": ["xlsx", "xlsm", "xltx", "xltm", "xlsb"], "support": "2.0; xlsb identity only"},
            {"driver": "powerpoint", "profiles": ["pptx", "pptm", "ppsx", "ppsm", "potx", "potm"], "support": "2.0"},
            {"driver": "keynote", "profiles": ["key"], "support": "2.0; modern IWA only"},
            {"driver": "numbers", "profiles": ["numbers"], "support": "2.0; modern IWA only"},
            {"driver": "pages", "profiles": ["pages"], "support": "2.0; modern IWA only"},
            {"driver": "office-legacy", "profiles": ["doc", "xls", "ppt", "encrypted-ooxml"], "support": "metadata and format statistics"}
        ]
    })
}

pub fn targets_report(format: &str) -> Result<Value> {
    const SELECTORS: [&str; 9] = [
        "@header",
        "@default",
        "@summary",
        "@security",
        "@structure",
        "@assets",
        "@quality",
        "@format",
        "@all",
    ];
    let driver = driver_for_name(format)?;
    let specs = driver.targets();
    let applicable_at_deep = driver.applicable_targets(ProbeLevel::Deep);
    let aliases = target_aliases(&specs);
    let mut aliases_by_target = BTreeMap::<String, Vec<String>>::new();
    for (alias, target) in aliases {
        aliases_by_target.entry(target).or_default().push(alias);
    }
    let targets = specs
        .iter()
        .map(|spec| {
            let mut value = serde_json::to_value(spec).expect("TargetSpec serializes");
            let object = value.as_object_mut().expect("TargetSpec is an object");
            object.insert(
                "aliases".to_owned(),
                json!(aliases_by_target.get(&spec.id).cloned().unwrap_or_default()),
            );
            object.insert("schema".to_owned(), target_value_schema(&spec.value_type));
            object.insert(
                "cost_class".to_owned(),
                json!(match spec.min_level {
                    ProbeLevel::Header => "low",
                    ProbeLevel::Metadata => "moderate",
                    ProbeLevel::Deep => "high",
                }),
            );
            object.insert(
                "applicable".to_owned(),
                json!(applicable_at_deep.contains(&spec.id)),
            );
            object.insert(
                "supported_levels".to_owned(),
                json!(
                    [ProbeLevel::Header, ProbeLevel::Metadata, ProbeLevel::Deep]
                        .into_iter()
                        .filter(|level| driver.applicable_targets(*level).contains(&spec.id))
                        .map(|level| match level {
                            ProbeLevel::Header => "header",
                            ProbeLevel::Metadata => "metadata",
                            ProbeLevel::Deep => "deep",
                        })
                        .collect::<Vec<_>>()
                ),
            );
            object.insert(
                "selectors".to_owned(),
                json!(
                    SELECTORS
                        .into_iter()
                        .filter(|selector| {
                            selector_targets(selector, &*driver, &specs, ProbeLevel::Deep)
                                .is_some_and(|targets| targets.contains(&spec.id))
                        })
                        .collect::<Vec<_>>()
                ),
            );
            value
        })
        .collect::<Vec<_>>();
    let selector_expansions = SELECTORS
        .into_iter()
        .map(|selector| {
            let levels = [ProbeLevel::Header, ProbeLevel::Metadata, ProbeLevel::Deep]
                .into_iter()
                .map(|level| {
                    let name = match level {
                        ProbeLevel::Header => "header",
                        ProbeLevel::Metadata => "metadata",
                        ProbeLevel::Deep => "deep",
                    };
                    let targets = selector_targets(selector, &*driver, &specs, level)
                        .unwrap_or_default()
                        .into_iter()
                        .collect::<Vec<_>>();
                    (name, targets)
                })
                .collect::<BTreeMap<_, _>>();
            (selector, levels)
        })
        .collect::<BTreeMap<_, _>>();
    Ok(json!({
        "schema_version": SCHEMA_VERSION,
        "tool_version": TOOL_VERSION,
        "status": "ok",
        "driver": driver.id(),
        "profile": driver.profile().profile,
        "targets": targets,
        "selector_expansions": selector_expansions,
        "format_options": driver.options(),
    }))
}

pub fn report_schema() -> Result<Value> {
    serde_json::from_str(include_str!("../../../docs/deckprobe-report.schema.json")).map_err(
        |error| DeckProbeError::Parser(format!("bundled report schema is invalid: {error}")),
    )
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope<'a> {
    schema_version: u32,
    tool_version: &'a str,
    status: &'static str,
    error: ErrorBody<'a>,
}

#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
    exit_code: u8,
}

pub fn error_report(error: &DeckProbeError) -> Value {
    serde_json::to_value(ErrorEnvelope {
        schema_version: SCHEMA_VERSION,
        tool_version: TOOL_VERSION,
        status: "error",
        error: ErrorBody {
            code: error_code(error),
            message: error.to_string(),
            exit_code: error_exit_code(error),
        },
    })
    .expect("error envelope serializes")
}

pub fn error_code(error: &DeckProbeError) -> &'static str {
    match error {
        DeckProbeError::Io(_) => "SOURCE_IO",
        DeckProbeError::BudgetExceeded(_) => "BUDGET_EXCEEDED",
        DeckProbeError::UnsupportedFormat(_) => "UNSUPPORTED_FORMAT",
        DeckProbeError::UnsupportedTarget(_) => "UNSUPPORTED_TARGET",
        DeckProbeError::InvalidRequest(_) => "INVALID_REQUEST",
        DeckProbeError::MalformedInput(_) => "MALFORMED_INPUT",
        DeckProbeError::Parser(_) => "PARSER_FAILURE",
    }
}

pub fn error_exit_code(error: &DeckProbeError) -> u8 {
    match error {
        DeckProbeError::InvalidRequest(_) | DeckProbeError::UnsupportedTarget(_) => 1,
        DeckProbeError::Io(_) => 2,
        DeckProbeError::UnsupportedFormat(_) => 3,
        DeckProbeError::MalformedInput(_) | DeckProbeError::BudgetExceeded(_) => 4,
        DeckProbeError::Parser(_) => 6,
    }
}

pub fn detect_driver(context: &mut ProbeContext) -> Result<Box<dyn FormatDriver>> {
    let extension = context.extension().ok_or_else(|| {
        DeckProbeError::UnsupportedFormat("input filename has no extension".to_owned())
    })?;
    let signature = context.read_prefix(8)?;
    let is_pdf = signature.starts_with(b"%PDF-");
    let is_zip = matches!(
        signature.get(..4),
        Some(b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08")
    );
    let is_cfb = signature == [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

    let mismatch = |expected: &str| {
        DeckProbeError::MalformedInput(format!(
            ".{extension} input does not contain the expected {expected} container"
        ))
    };
    Ok(match extension.as_str() {
        "pdf" if is_pdf => Box::new(PdfDriver::new()),
        "pdf" => return Err(mismatch("PDF")),
        "doc" | "dot" | "xls" | "xlt" | "ppt" | "pps" | "pot" if is_cfb => Box::new(
            OfficeLegacyDriver::new(profile_for_extension(Some(&extension))),
        ),
        "doc" | "dot" | "xls" | "xlt" | "ppt" | "pps" | "pot" => {
            return Err(mismatch("CFB/OLE"));
        }
        "docx" | "docm" | "dotx" | "dotm" | "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" | "pptx"
        | "pptm" | "ppsx" | "ppsm" | "potx" | "potm"
            if is_cfb =>
        {
            Box::new(OfficeLegacyDriver::new(
                deckprobe_format_office_legacy::ENCRYPTED_OOXML,
            ))
        }
        "docx" if is_zip => Box::new(WordDriver::new(deckprobe_format_ooxml::DOCX)),
        "docm" if is_zip => Box::new(WordDriver::new(deckprobe_format_ooxml::DOCM)),
        "dotx" if is_zip => Box::new(WordDriver::new(deckprobe_format_ooxml::DOTX)),
        "dotm" if is_zip => Box::new(WordDriver::new(deckprobe_format_ooxml::DOTM)),
        "xlsx" if is_zip => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLSX)),
        "xlsm" if is_zip => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLSM)),
        "xltx" if is_zip => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLTX)),
        "xltm" if is_zip => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLTM)),
        "xlsb" if is_zip => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLSB)),
        "pptx" if is_zip => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::PPTX)),
        "pptm" if is_zip => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::PPTM)),
        "ppsx" if is_zip => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::PPSX)),
        "ppsm" if is_zip => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::PPSM)),
        "potx" if is_zip => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::POTX)),
        "potm" if is_zip => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::POTM)),
        "key" if is_zip => Box::new(IworkDriver::open(context, IworkKind::Keynote)?),
        "numbers" if is_zip => Box::new(IworkDriver::open(context, IworkKind::Numbers)?),
        "pages" if is_zip => Box::new(IworkDriver::open(context, IworkKind::Pages)?),
        "key" | "numbers" | "pages" => return Err(mismatch("ZIP/IWA")),
        "docx" | "docm" | "dotx" | "dotm" | "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" | "pptx"
        | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" => {
            return Err(mismatch("ZIP/OPC or encrypted CFB"));
        }
        _ => return Err(DeckProbeError::UnsupportedFormat(format!(".{extension}"))),
    })
}

pub fn driver_for_name(name: &str) -> Result<Box<dyn FormatDriver>> {
    let name = name.to_ascii_lowercase();
    Ok(match name.as_str() {
        "pdf" => Box::new(PdfDriver::new()),
        "word" | "docx" => Box::new(WordDriver::new(deckprobe_format_ooxml::DOCX)),
        "docm" => Box::new(WordDriver::new(deckprobe_format_ooxml::DOCM)),
        "dotx" => Box::new(WordDriver::new(deckprobe_format_ooxml::DOTX)),
        "dotm" => Box::new(WordDriver::new(deckprobe_format_ooxml::DOTM)),
        "excel" | "xlsx" => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLSX)),
        "xlsm" => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLSM)),
        "xltx" => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLTX)),
        "xltm" => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLTM)),
        "xlsb" => Box::new(ExcelDriver::new(deckprobe_format_ooxml::XLSB)),
        "powerpoint" | "pptx" => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::PPTX)),
        "pptm" => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::PPTM)),
        "ppsx" => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::PPSX)),
        "ppsm" => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::PPSM)),
        "potx" => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::POTX)),
        "potm" => Box::new(PowerPointDriver::new(deckprobe_format_ooxml::POTM)),
        "keynote" | "key" => Box::new(IworkDriver::new(IworkKind::Keynote)),
        "numbers" => Box::new(IworkDriver::new(IworkKind::Numbers)),
        "pages" => Box::new(IworkDriver::new(IworkKind::Pages)),
        "legacy" | "ppt" => Box::new(OfficeLegacyDriver::new(
            deckprobe_format_office_legacy::LEGACY_PPT,
        )),
        "doc" => Box::new(OfficeLegacyDriver::new(
            deckprobe_format_office_legacy::LEGACY_DOC,
        )),
        "xls" => Box::new(OfficeLegacyDriver::new(
            deckprobe_format_office_legacy::LEGACY_XLS,
        )),
        _ => return Err(DeckProbeError::UnsupportedFormat(name)),
    })
}

fn forced_format_matches(forced: &str, driver: &dyn FormatDriver) -> bool {
    forced == driver.id()
        || forced == driver.profile().profile
        || (forced == "legacy" && driver.id() == "office-legacy")
        || (forced == "word" && driver.profile().driver == "word")
        || (forced == "excel" && driver.profile().driver == "excel")
        || (forced == "powerpoint" && driver.profile().driver == "powerpoint")
        || (forced == "iwork" && matches!(driver.id(), "keynote" | "numbers" | "pages"))
}

fn expand_targets(
    values: &[String],
    driver: &dyn FormatDriver,
    specs: &[TargetSpec],
    level: ProbeLevel,
) -> Result<BTreeSet<String>> {
    let known = specs
        .iter()
        .map(|spec| spec.id.as_str())
        .collect::<BTreeSet<_>>();
    let aliases = target_aliases(specs);
    let mut targets = BTreeSet::new();
    for selector in values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        match selector_targets(selector, driver, specs, level) {
            Some(selected) => targets.extend(selected),
            None if known.contains(selector) => {
                targets.insert(selector.to_owned());
            }
            None if aliases.contains_key(selector) => {
                targets.insert(aliases[selector].clone());
            }
            None => return Err(DeckProbeError::UnsupportedTarget(selector.to_owned())),
        }
    }
    if targets.is_empty() {
        return Err(DeckProbeError::InvalidRequest(
            "target set is empty".to_owned(),
        ));
    }
    Ok(targets)
}

fn selector_targets(
    selector: &str,
    driver: &dyn FormatDriver,
    specs: &[TargetSpec],
    level: ProbeLevel,
) -> Option<BTreeSet<String>> {
    let applicable = driver.applicable_targets(level);
    let available = |predicate: &dyn Fn(&TargetSpec) -> bool| {
        specs
            .iter()
            .filter(|spec| {
                spec.min_level <= level && applicable.contains(&spec.id) && predicate(spec)
            })
            .map(|spec| spec.id.clone())
            .collect::<BTreeSet<_>>()
    };
    match selector {
        "@header" => Some(driver.default_targets(ProbeLevel::Header)),
        "@default" => Some(driver.default_targets(level)),
        "@summary" => Some(summary_targets(driver, specs, level)),
        "@security" => Some(available(&|spec| spec.id.starts_with("security."))),
        "@structure" => Some(available(&|spec| is_structure_target(&spec.id))),
        "@assets" => Some(available(&|spec| is_asset_target(&spec.id))),
        "@quality" => Some(available(&|spec| is_quality_target(&spec.id))),
        "@format" => Some(available(&|spec| spec.scope == TargetScope::Format)),
        "@all" => Some(available(&|_| true)),
        _ => None,
    }
}

fn summary_targets(
    driver: &dyn FormatDriver,
    specs: &[TargetSpec],
    level: ProbeLevel,
) -> BTreeSet<String> {
    let applicable = driver.applicable_targets(level);
    let known = specs
        .iter()
        .filter(|spec| spec.min_level <= level && applicable.contains(&spec.id))
        .map(|spec| spec.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut targets = driver.default_targets(ProbeLevel::Header);
    if level < ProbeLevel::Metadata {
        return targets;
    }
    let mut requested = vec![
        "document.title",
        "document.author",
        "document.application",
        "document.application_version",
    ];
    match driver.profile().profile {
        "pdf" => {}
        "docx" | "docm" | "dotx" | "dotm" => {
            requested.extend(["word.page_count", "word.word_count", "word.is_template"])
        }
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" => requested.extend([
            "excel.sheet_count",
            "excel.sheet_names",
            "excel.is_template",
            "excel.binary_workbook",
        ]),
        "pptx" | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" => requested.extend([
            "powerpoint.slide_count",
            "powerpoint.slide_size",
            "powerpoint.aspect_ratio",
            "powerpoint.orientation",
            "powerpoint.presentation_kind",
        ]),
        "key" => requested.extend([
            "keynote.slide_count",
            "keynote.slide_size",
            "keynote.aspect_ratio",
            "keynote.orientation",
            "keynote.hidden_slide_count",
            "iwork.has_preview",
        ]),
        "numbers" => requested.extend([
            "numbers.sheet_count",
            "numbers.sheet_names",
            "numbers.table_count",
            "iwork.has_preview",
        ]),
        "pages" => requested.extend([
            "pages.section_count",
            "pages.page_size",
            "pages.aspect_ratio",
            "pages.orientation",
            "pages.cached_page_count",
            "iwork.is_multi_page",
            "iwork.has_preview",
        ]),
        "doc" | "xls" | "ppt" | "encrypted-ooxml" => requested.extend(["office.cfb_entry_count"]),
        _ => {}
    }
    targets.extend(
        requested
            .into_iter()
            .filter(|target| known.contains(target))
            .map(str::to_owned),
    );
    targets
}

fn is_structure_target(target: &str) -> bool {
    [
        "_count",
        "_names",
        ".slide_size",
        ".page_size",
        ".table_dimensions",
        ".body_text_length",
        ".aspect_ratio",
        ".orientation",
        ".presentation_kind",
        ".is_template",
        ".binary_workbook",
        ".document_kind",
    ]
    .iter()
    .any(|marker| target.contains(marker))
}

fn is_asset_target(target: &str) -> bool {
    if target.starts_with("security.") {
        return false;
    }
    [
        "asset",
        "preview",
        "image",
        "media",
        "font",
        "attachment",
        "embedded",
    ]
    .iter()
    .any(|marker| target.contains(marker))
}

fn is_quality_target(target: &str) -> bool {
    target.starts_with("quality.")
        || matches!(
            target,
            "document.extension_matches" | "office.conformance" | "pdf.repaired"
        )
}

fn target_aliases(specs: &[TargetSpec]) -> BTreeMap<String, String> {
    let mut candidates = BTreeMap::<String, Vec<String>>::new();
    for spec in specs {
        if let Some((_, suffix)) = spec.id.rsplit_once('.') {
            candidates
                .entry(suffix.to_owned())
                .or_default()
                .push(spec.id.clone());
        }
    }
    let mut aliases = candidates
        .into_iter()
        .filter_map(|(alias, targets)| (targets.len() == 1).then(|| (alias, targets[0].clone())))
        .collect::<BTreeMap<_, _>>();
    let known = specs
        .iter()
        .map(|spec| spec.id.as_str())
        .collect::<BTreeSet<_>>();
    for (alias, target) in [
        ("mime", "document.mime_type"),
        ("profile", "document.format_profile"),
        ("macros", "security.has_macros"),
        ("notes_count", "powerpoint.notes_slide_count"),
    ] {
        if known.contains(target) {
            aliases.insert(alias.to_owned(), target.to_owned());
        }
    }
    aliases
}

fn resolve_format_options(
    values: &BTreeMap<String, String>,
    driver: &dyn FormatDriver,
) -> Result<BTreeMap<String, String>> {
    let mut options = BTreeMap::new();
    let supported = driver.options();
    for (key, setting) in values {
        if key.is_empty() || setting.is_empty() {
            return Err(DeckProbeError::InvalidRequest(format!(
                "invalid format option: {key}={setting}"
            )));
        }
        let normalized = if key.contains('.') {
            key.to_owned()
        } else {
            let matches = supported
                .iter()
                .filter(|option| option.key.rsplit_once('.').map(|(_, suffix)| suffix) == Some(key))
                .map(|option| option.key.clone())
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [candidate] => candidate.clone(),
                [] => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "unknown {} option: {key}",
                        driver.id()
                    )));
                }
                _ => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "ambiguous {} option {key}; use a namespaced key",
                        driver.id()
                    )));
                }
            }
        };
        options.insert(normalized, setting.clone());
    }
    Ok(options)
}

fn resolve_target_confidence(
    values: &BTreeMap<String, Confidence>,
    specs: &[TargetSpec],
    requested: &BTreeSet<String>,
) -> Result<BTreeMap<String, Confidence>> {
    let aliases = target_aliases(specs);
    let known = specs
        .iter()
        .map(|spec| spec.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut overrides = BTreeMap::new();
    for (target, confidence) in values {
        let target = if known.contains(target.as_str()) {
            target.to_owned()
        } else if let Some(target) = aliases.get(target) {
            target.clone()
        } else {
            return Err(DeckProbeError::UnsupportedTarget(target.to_owned()));
        };
        if !requested.contains(&target) {
            return Err(DeckProbeError::InvalidRequest(format!(
                "target confidence override refers to unrequested target {target}"
            )));
        }
        if *confidence == Confidence::None {
            return Err(DeckProbeError::InvalidRequest(format!(
                "target confidence cannot be none: {target}"
            )));
        }
        overrides.insert(target, *confidence);
    }
    Ok(overrides)
}

fn merge_results(
    requested: &BTreeSet<String>,
    unresolved: &[String],
    evidence: Vec<Evidence>,
    plan_only: bool,
    request: &ProbeRequest,
) -> (BTreeMap<String, Evidence>, Vec<Diagnostic>, Vec<String>) {
    let mut results = BTreeMap::new();
    for item in evidence
        .into_iter()
        .filter(|item| requested.contains(&item.target))
    {
        let replace = results
            .get(&item.target)
            .map(|current: &Evidence| item.confidence > current.confidence)
            .unwrap_or(true);
        if replace {
            results.insert(item.target.clone(), item);
        }
    }
    for target in requested {
        results.entry(target.clone()).or_insert_with(|| {
            Evidence::unresolved(
                target,
                if plan_only {
                    TargetStatus::Planned
                } else if unresolved.contains(target) {
                    TargetStatus::Unsupported
                } else {
                    TargetStatus::Unknown
                },
                if plan_only { "plan-only" } else { "planner" },
            )
        });
    }
    let mut unsatisfied = unresolved.to_vec();
    let mut diagnostics = unresolved
        .iter()
        .map(|target| Diagnostic {
            level: "warning".to_owned(),
            code: "UNSUPPORTED_TARGET_AT_REQUESTED_LEVEL".to_owned(),
            message: format!("no path satisfies {target} at requested level/confidence"),
        })
        .collect::<Vec<_>>();
    if !plan_only {
        for (target, result) in results
            .iter()
            .filter(|(target, _)| request.targets.contains(*target))
        {
            let minimum_confidence = request.minimum_confidence_for(target);
            let has_value = matches!(
                result.status,
                TargetStatus::Resolved | TargetStatus::Estimated
            );
            if (!has_value || result.confidence < minimum_confidence)
                && !unsatisfied.contains(target)
            {
                unsatisfied.push(target.clone());
                if has_value {
                    diagnostics.push(Diagnostic {
                        level: "warning".to_owned(),
                        code: "INSUFFICIENT_CONFIDENCE".to_owned(),
                        message: format!(
                            "{target} resolved at {:?}, below requested {:?}",
                            result.confidence, minimum_confidence
                        )
                        .to_ascii_lowercase(),
                    });
                } else {
                    diagnostics.push(Diagnostic {
                        level: "warning".to_owned(),
                        code: "UNRESOLVED_TARGET".to_owned(),
                        message: format!("{target} could not be resolved from this document"),
                    });
                }
            }
        }
    }
    unsatisfied.sort();
    unsatisfied.dedup();
    (results, diagnostics, unsatisfied)
}

fn target_value_schema(value_type: &str) -> Value {
    match value_type {
        "string" => json!({"type": "string"}),
        "string|null" => json!({"type": ["string", "null"]}),
        "bool" => json!({"type": "boolean"}),
        "bool|null" => json!({"type": ["boolean", "null"]}),
        "u64" => json!({"type": "integer", "minimum": 0}),
        "u64|null" => json!({"type": ["integer", "null"], "minimum": 0}),
        "string[]|null" | "array<string>|null" => json!({
            "type": ["array", "null"],
            "items": {"type": "string"}
        }),
        "string[]" | "array<string>" => json!({
            "type": "array",
            "items": {"type": "string"}
        }),
        "array<object>" => json!({
            "type": "array",
            "items": {"type": "object"}
        }),
        "object" => json!({"type": "object"}),
        "object|null" => json!({"type": ["object", "null"]}),
        _ => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use deckprobe_core::{MemorySource, ProbeLevel, ProbeOptions};
    use serde_json::json;

    use super::{formats_report, probe_source, targets_report};

    #[test]
    fn probes_memory_bytes_without_a_file_api() {
        let options = ProbeOptions {
            targets: vec!["@header".to_owned()],
            level: ProbeLevel::Header,
            ..ProbeOptions::default()
        };
        let report = probe_source(
            MemorySource::with_kind("browser.pdf", "browser_bytes", &b"%PDF-1.7\n"[..]),
            options,
        )
        .expect("memory probe");
        assert_eq!(report.input.source_kind, "browser_bytes");
        assert_eq!(report.driver.id, "pdf");
        assert_eq!(report.results["pdf.version"].value.as_ref().unwrap(), "1.7");
    }

    #[test]
    fn discovery_is_owned_by_the_engine() {
        assert_eq!(formats_report()["status"], "ok");
        let targets = targets_report("pptx").expect("targets");
        assert_eq!(targets["driver"], "powerpoint");
        assert!(targets["selector_expansions"]["@security"]["metadata"].is_array());
    }

    #[test]
    fn iwork_selectors_only_expose_executable_targets() {
        let targets = targets_report("key").expect("keynote targets");
        let security = targets["selector_expansions"]["@security"]["metadata"]
            .as_array()
            .expect("security expansion");
        assert_eq!(
            security,
            &vec![
                json!("security.encrypted"),
                json!("security.has_macros"),
                json!("security.password_protected"),
            ]
        );

        let summary = targets["selector_expansions"]["@summary"]["metadata"]
            .as_array()
            .expect("summary expansion");
        assert!(!summary.contains(&json!("document.title")));
        assert!(!summary.contains(&json!("document.author")));
        assert!(summary.contains(&json!("document.application_version")));

        let quality = targets["selector_expansions"]["@quality"]["deep"]
            .as_array()
            .expect("quality expansion");
        assert!(quality.contains(&json!("quality.corrupted")));
        assert!(!quality.contains(&json!("quality.missing_assets")));

        let title = targets["targets"]
            .as_array()
            .expect("target catalog")
            .iter()
            .find(|target| target["id"] == "document.title")
            .expect("title target");
        assert_eq!(title["applicable"], false);
        assert_eq!(title["supported_levels"], json!([]));

        let producer_build = targets["targets"]
            .as_array()
            .expect("target catalog")
            .iter()
            .find(|target| target["id"] == "iwork.producer_build")
            .expect("producer build target");
        assert_eq!(producer_build["applicable"], true);
        assert_eq!(
            producer_build["supported_levels"],
            json!(["metadata", "deep"])
        );

        let slide_size = targets["targets"]
            .as_array()
            .expect("target catalog")
            .iter()
            .find(|target| target["id"] == "keynote.slide_size")
            .expect("Keynote deep target");
        assert_eq!(slide_size["applicable"], true);
        assert_eq!(slide_size["supported_levels"], json!(["deep"]));
        assert!(
            targets["selector_expansions"]["@summary"]["deep"]
                .as_array()
                .expect("deep summary")
                .contains(&json!("keynote.slide_size"))
        );

        let numbers = targets_report("numbers").expect("Numbers targets");
        assert!(
            numbers["selector_expansions"]["@structure"]["deep"]
                .as_array()
                .expect("Numbers structure")
                .contains(&json!("numbers.table_dimensions"))
        );

        let pages = targets_report("pages").expect("Pages targets");
        assert!(
            pages["selector_expansions"]["@summary"]["deep"]
                .as_array()
                .expect("Pages deep summary")
                .contains(&json!("pages.cached_page_count"))
        );
    }
}
