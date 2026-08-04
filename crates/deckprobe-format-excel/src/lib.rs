use std::collections::BTreeSet;

use deckprobe_core::{
    Confidence, DeckProbeError, Evidence, ExecutionPlan, FormatDriver, FormatProfile, OptionSpec,
    PathDescriptor, ProbeContext, ProbeLevel, ProbeRequest, Result, TargetScope, TargetSpec,
    common_target_specs,
};
use deckprobe_format_ooxml::{
    OoxmlSession, app_properties_targets, common_path_targets, core_properties_targets,
    count_unique_parts, deep_security_targets, first_element_attributes, identity_path_evidence,
    inventory_targets, office_target_specs, package_security_targets, readability_security_targets,
    run_common_path, start_elements_with_attributes,
};
use serde_json::json;

pub struct ExcelDriver {
    profile: FormatProfile,
}

impl ExcelDriver {
    pub fn new(profile: FormatProfile) -> Self {
        Self { profile }
    }

    fn excel_targets() -> Vec<TargetSpec> {
        use ProbeLevel::{Deep, Metadata};
        use TargetScope::Format;
        vec![
            TargetSpec::new(
                "excel.sheet_count",
                "Workbook sheet count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.sheet_names",
                "Workbook sheet names in order",
                "string[]|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.hidden_sheet_count",
                "Hidden and veryHidden sheet count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.defined_name_count",
                "Workbook defined-name count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.shared_string_count",
                "Unique shared string count",
                "u64|null",
                Format,
                Deep,
            ),
            TargetSpec::new(
                "excel.table_count",
                "Native table part count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.is_template",
                "Whether profile is an Excel template",
                "bool",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.binary_workbook",
                "Whether workbook main part is binary",
                "bool",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.chart_part_count",
                "Unique Excel chart XML part count (not chart instance count)",
                "u64",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.pivot_table_part_count",
                "Unique Excel pivot-table definition part count (not pivot instance count)",
                "u64",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "excel.unique_image_asset_count",
                "Unique image asset part count in the Excel package (not image instance count)",
                "u64",
                Format,
                Metadata,
            ),
        ]
    }

    fn workbook_path(request: &ProbeRequest) -> &str {
        request
            .format_options
            .get("excel.workbook_path")
            .map(String::as_str)
            .unwrap_or("auto")
    }
}

impl FormatDriver for ExcelDriver {
    fn id(&self) -> &'static str {
        "excel"
    }

    fn profile(&self) -> &FormatProfile {
        &self.profile
    }

    fn targets(&self) -> Vec<TargetSpec> {
        let mut targets = common_target_specs();
        targets.extend(office_target_specs());
        targets.extend(Self::excel_targets());
        targets
    }

    fn options(&self) -> Vec<OptionSpec> {
        vec![OptionSpec {
            key: "excel.workbook_path".to_owned(),
            description: "Select workbook.xml exact path or faster inventory estimate".to_owned(),
            value_type: "enum".to_owned(),
            default: "auto".to_owned(),
            allowed: vec!["auto".to_owned(), "xml".to_owned(), "inventory".to_owned()],
        }]
    }

    fn default_targets(&self, level: ProbeLevel) -> BTreeSet<String> {
        let mut targets = common_path_targets()
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<BTreeSet<_>>();
        if level >= ProbeLevel::Metadata {
            targets.extend(
                [
                    "document.title",
                    "document.author",
                    "document.application",
                    "security.has_macros",
                    "office.package_entry_count",
                    "excel.is_template",
                    "excel.binary_workbook",
                    "excel.table_count",
                ]
                .into_iter()
                .map(str::to_owned),
            );
            if self.profile.profile != "xlsb" {
                targets.extend(
                    [
                        "excel.sheet_count",
                        "excel.sheet_names",
                        "excel.hidden_sheet_count",
                    ]
                    .into_iter()
                    .map(str::to_owned),
                );
            }
        }
        if level >= ProbeLevel::Deep && self.profile.profile != "xlsb" {
            targets.insert("excel.shared_string_count".to_owned());
        }
        targets
    }

    fn paths(&self, request: &ProbeRequest) -> Result<Vec<PathDescriptor>> {
        self.validate_options(request)?;
        let mut paths = vec![
            PathDescriptor::new(
                "ooxml.identity",
                common_path_targets(),
                ProbeLevel::Header,
                Confidence::Exact,
                1,
            ),
            PathDescriptor::new(
                "ooxml.core_properties",
                core_properties_targets(),
                ProbeLevel::Metadata,
                Confidence::High,
                8,
            ),
            PathDescriptor::new(
                "ooxml.app_properties",
                app_properties_targets(),
                ProbeLevel::Metadata,
                Confidence::High,
                8,
            ),
            PathDescriptor::new(
                "ooxml.package_inventory",
                inventory_targets(),
                ProbeLevel::Metadata,
                Confidence::Exact,
                4,
            ),
            PathDescriptor::new(
                "ooxml.readability_security",
                readability_security_targets(),
                ProbeLevel::Metadata,
                Confidence::Exact,
                1,
            ),
            PathDescriptor::new(
                "ooxml.package_security",
                package_security_targets(),
                ProbeLevel::Metadata,
                Confidence::Exact,
                12,
            ),
            PathDescriptor::new(
                "ooxml.deep_security",
                deep_security_targets(),
                ProbeLevel::Deep,
                Confidence::Exact,
                14,
            ),
            PathDescriptor::new(
                "excel.profile",
                &["excel.is_template", "excel.binary_workbook"],
                ProbeLevel::Metadata,
                Confidence::Exact,
                1,
            ),
            PathDescriptor::new(
                "excel.table_inventory",
                &["excel.table_count"],
                ProbeLevel::Metadata,
                Confidence::Exact,
                2,
            ),
            PathDescriptor::new(
                "excel.asset_inventory",
                &[
                    "excel.chart_part_count",
                    "excel.pivot_table_part_count",
                    "excel.unique_image_asset_count",
                ],
                ProbeLevel::Metadata,
                Confidence::Exact,
                2,
            ),
        ];
        if self.profile.profile != "xlsb" {
            if matches!(Self::workbook_path(request), "auto" | "xml") {
                paths.push(PathDescriptor::new(
                    "excel.workbook_xml",
                    &[
                        "excel.sheet_count",
                        "excel.sheet_names",
                        "excel.hidden_sheet_count",
                        "excel.defined_name_count",
                    ],
                    ProbeLevel::Metadata,
                    Confidence::Exact,
                    12,
                ));
            }
            if matches!(Self::workbook_path(request), "auto" | "inventory") {
                paths.push(PathDescriptor::new(
                    "excel.worksheet_inventory",
                    &["excel.sheet_count"],
                    ProbeLevel::Metadata,
                    Confidence::Medium,
                    2,
                ));
            }
            paths.push(PathDescriptor::new(
                "excel.shared_strings",
                &["excel.shared_string_count"],
                ProbeLevel::Deep,
                Confidence::Exact,
                20,
            ));
        }
        Ok(paths)
    }

    fn validate_options(&self, request: &ProbeRequest) -> Result<()> {
        for (key, value) in &request.format_options {
            if key != "excel.workbook_path" {
                return Err(DeckProbeError::InvalidRequest(format!(
                    "unknown Excel option: {key}"
                )));
            }
            if !["auto", "xml", "inventory"].contains(&value.as_str()) {
                return Err(DeckProbeError::InvalidRequest(format!(
                    "invalid {key}: {value}"
                )));
            }
        }
        Ok(())
    }

    fn execute(
        &self,
        context: &mut ProbeContext,
        _request: &ProbeRequest,
        plan: &ExecutionPlan,
    ) -> Result<Vec<Evidence>> {
        let mut session = Some(OoxmlSession::open(context)?);
        session
            .as_mut()
            .expect("package session")
            .validate_profile(context, &self.profile)?;
        let mut output = Vec::new();
        for path in &plan.paths {
            context.check_time()?;
            match path.as_str() {
                "ooxml.identity" => output.extend(identity_path_evidence(context, &self.profile)),
                "ooxml.core_properties"
                | "ooxml.app_properties"
                | "ooxml.package_inventory"
                | "ooxml.readability_security"
                | "ooxml.package_security"
                | "ooxml.deep_security" => {
                    output.extend(run_common_path(
                        path,
                        session.as_mut().expect("package session"),
                        context,
                        &self.profile,
                    )?);
                }
                "excel.profile" => {
                    output.push(Evidence::resolved(
                        "excel.is_template",
                        matches!(self.profile.profile, "xltx" | "xltm"),
                        Confidence::Exact,
                        path,
                        "detected OOXML profile",
                    ));
                    output.push(Evidence::resolved(
                        "excel.binary_workbook",
                        self.profile.profile == "xlsb",
                        Confidence::Exact,
                        path,
                        "detected OOXML profile",
                    ));
                }
                "excel.table_inventory" => {
                    let count = session
                        .as_ref()
                        .expect("package session")
                        .entry_names()
                        .iter()
                        .filter(|name| name.starts_with("xl/tables/") && name.ends_with(".xml"))
                        .count();
                    output.push(Evidence::resolved(
                        "excel.table_count",
                        json!(count),
                        Confidence::Exact,
                        path,
                        "OPC entry inventory",
                    ));
                }
                "excel.asset_inventory" => {
                    let image_count = session
                        .as_mut()
                        .expect("package session")
                        .unique_image_asset_part_count(context, "xl/media/")?;
                    let names = session.as_ref().expect("package session").entry_names();
                    output.extend([
                        Evidence::resolved(
                            "excel.chart_part_count",
                            json!(count_unique_parts(names, "xl/charts/chart", ".xml")),
                            Confidence::Exact,
                            path,
                            "unique Excel chart XML parts",
                        ),
                        Evidence::resolved(
                            "excel.pivot_table_part_count",
                            json!(count_unique_parts(
                                names,
                                "xl/pivotTables/pivotTable",
                                ".xml"
                            )),
                            Confidence::Exact,
                            path,
                            "unique Excel pivot-table definition parts",
                        ),
                        Evidence::resolved(
                            "excel.unique_image_asset_count",
                            json!(image_count),
                            Confidence::Exact,
                            path,
                            "unique image asset parts under xl/media/",
                        ),
                    ]);
                }
                "excel.worksheet_inventory" => {
                    let count = session
                        .as_ref()
                        .expect("package session")
                        .entry_names()
                        .iter()
                        .filter(|name| {
                            name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml")
                        })
                        .count();
                    output.push(Evidence::resolved(
                        "excel.sheet_count",
                        json!(count),
                        Confidence::Medium,
                        path,
                        "worksheet part inventory",
                    ));
                }
                "excel.workbook_xml" => {
                    let xml = session
                        .as_mut()
                        .expect("package session")
                        .read_text(context, "xl/workbook.xml")?
                        .ok_or_else(|| {
                            DeckProbeError::MalformedInput("missing xl/workbook.xml".to_owned())
                        })?;
                    let sheets = start_elements_with_attributes(&xml, "sheet");
                    let names = sheets
                        .iter()
                        .filter_map(|attrs| attrs.get("name").cloned())
                        .collect::<Vec<_>>();
                    let hidden = sheets
                        .iter()
                        .filter(|attrs| attrs.get("state").is_some_and(|value| value != "visible"))
                        .count();
                    let defined_names =
                        deckprobe_format_ooxml::count_start_elements(&xml, "definedname");
                    output.extend([
                        Evidence::resolved(
                            "excel.sheet_count",
                            json!(sheets.len()),
                            Confidence::Exact,
                            path,
                            "xl/workbook.xml",
                        ),
                        Evidence::resolved(
                            "excel.sheet_names",
                            json!(names),
                            Confidence::Exact,
                            path,
                            "xl/workbook.xml",
                        ),
                        Evidence::resolved(
                            "excel.hidden_sheet_count",
                            json!(hidden),
                            Confidence::Exact,
                            path,
                            "xl/workbook.xml",
                        ),
                        Evidence::resolved(
                            "excel.defined_name_count",
                            json!(defined_names),
                            Confidence::Exact,
                            path,
                            "xl/workbook.xml",
                        ),
                    ]);
                }
                "excel.shared_strings" => {
                    let value = session
                        .as_mut()
                        .expect("package session")
                        .read_text(context, "xl/sharedStrings.xml")?;
                    match value
                        .and_then(|xml| first_element_attributes(&xml, "sst"))
                        .and_then(|attributes| {
                            attributes
                                .get("uniquecount")
                                .or_else(|| attributes.get("count"))
                                .cloned()
                        })
                        .and_then(|value| value.parse::<u64>().ok())
                    {
                        Some(count) => output.push(Evidence::resolved(
                            "excel.shared_string_count",
                            json!(count),
                            Confidence::Exact,
                            path,
                            "xl/sharedStrings.xml",
                        )),
                        None => output.push(Evidence::unresolved(
                            "excel.shared_string_count",
                            deckprobe_core::TargetStatus::Unknown,
                            path,
                        )),
                    }
                }
                other => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "unknown Excel path: {other}"
                    )));
                }
            }
        }
        Ok(output)
    }
}
