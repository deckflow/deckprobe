use std::collections::BTreeSet;

use deckprobe_core::{
    Confidence, DeckProbeError, Evidence, ExecutionPlan, FormatDriver, FormatProfile, OptionSpec,
    PathDescriptor, ProbeContext, ProbeLevel, ProbeRequest, Result, TargetScope, TargetSpec,
    common_target_specs,
};
use deckprobe_format_ooxml::{
    OoxmlSession, app_properties_targets, common_path_targets, core_properties_targets,
    count_start_elements, count_unique_parts, deep_security_targets, element_text_map,
    identity_path_evidence, inventory_targets, office_target_specs, package_security_targets,
    readability_security_targets, run_common_path,
};
use serde_json::json;

pub struct WordDriver {
    profile: FormatProfile,
}

impl WordDriver {
    pub fn new(profile: FormatProfile) -> Self {
        Self { profile }
    }

    fn word_targets() -> Vec<TargetSpec> {
        use ProbeLevel::{Deep, Metadata};
        use TargetScope::Format;
        vec![
            TargetSpec::new(
                "word.page_count",
                "Last saved page count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "word.word_count",
                "Last saved word count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "word.character_count",
                "Last saved character count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "word.paragraph_count",
                "Paragraph count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "word.table_count",
                "Native Word table count",
                "u64|null",
                Format,
                Deep,
            ),
            TargetSpec::new(
                "word.is_template",
                "Whether profile is a Word template",
                "bool",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "word.unique_image_asset_count",
                "Unique image asset part count in the Word package (not image instance count)",
                "u64",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "word.comment_part_count",
                "Unique Word comment-related XML part count (not logical comment count)",
                "u64",
                Format,
                Metadata,
            ),
        ]
    }

    fn stats_path(request: &ProbeRequest) -> &str {
        request
            .format_options
            .get("word.statistics_path")
            .map(String::as_str)
            .unwrap_or("auto")
    }
}

impl FormatDriver for WordDriver {
    fn id(&self) -> &'static str {
        "word"
    }

    fn profile(&self) -> &FormatProfile {
        &self.profile
    }

    fn targets(&self) -> Vec<TargetSpec> {
        let mut targets = common_target_specs();
        targets.extend(office_target_specs());
        targets.extend(Self::word_targets());
        targets
    }

    fn options(&self) -> Vec<OptionSpec> {
        vec![OptionSpec {
            key: "word.statistics_path".to_owned(),
            description: "Choose fast saved properties or exact document XML where available"
                .to_owned(),
            value_type: "enum".to_owned(),
            default: "auto".to_owned(),
            allowed: vec![
                "auto".to_owned(),
                "app-properties".to_owned(),
                "document-xml".to_owned(),
            ],
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
                    "word.page_count",
                    "word.word_count",
                    "word.is_template",
                ]
                .into_iter()
                .map(str::to_owned),
            );
        }
        if level >= ProbeLevel::Deep {
            targets.extend(
                ["word.paragraph_count", "word.table_count"]
                    .into_iter()
                    .map(str::to_owned),
            );
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
                "word.profile",
                &["word.is_template"],
                ProbeLevel::Metadata,
                Confidence::Exact,
                1,
            ),
            PathDescriptor::new(
                "word.asset_inventory",
                &["word.unique_image_asset_count", "word.comment_part_count"],
                ProbeLevel::Metadata,
                Confidence::Exact,
                2,
            ),
        ];
        if matches!(Self::stats_path(request), "auto" | "app-properties") {
            paths.push(PathDescriptor::new(
                "word.app_statistics",
                &[
                    "word.page_count",
                    "word.word_count",
                    "word.character_count",
                    "word.paragraph_count",
                ],
                ProbeLevel::Metadata,
                Confidence::High,
                8,
            ));
        }
        if matches!(Self::stats_path(request), "auto" | "document-xml") {
            paths.push(PathDescriptor::new(
                "word.document_structure",
                &["word.paragraph_count", "word.table_count"],
                ProbeLevel::Deep,
                Confidence::Exact,
                40,
            ));
        }
        Ok(paths)
    }

    fn validate_options(&self, request: &ProbeRequest) -> Result<()> {
        for (key, value) in &request.format_options {
            if key != "word.statistics_path" {
                return Err(DeckProbeError::InvalidRequest(format!(
                    "unknown Word option: {key}"
                )));
            }
            if !["auto", "app-properties", "document-xml"].contains(&value.as_str()) {
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
                "word.profile" => output.push(Evidence::resolved(
                    "word.is_template",
                    matches!(self.profile.profile, "dotx" | "dotm"),
                    Confidence::Exact,
                    path,
                    "detected OOXML profile",
                )),
                "word.asset_inventory" => {
                    let image_count = session
                        .as_mut()
                        .expect("package session")
                        .unique_image_asset_part_count(context, "word/media/")?;
                    let comment_count = count_unique_parts(
                        session.as_ref().expect("package session").entry_names(),
                        "word/comments",
                        ".xml",
                    );
                    output.extend([
                        Evidence::resolved(
                            "word.unique_image_asset_count",
                            json!(image_count),
                            Confidence::Exact,
                            path,
                            "unique image asset parts under word/media/",
                        ),
                        Evidence::resolved(
                            "word.comment_part_count",
                            json!(comment_count),
                            Confidence::Exact,
                            path,
                            "unique Word comment-related XML parts",
                        ),
                    ]);
                }
                "word.app_statistics" => {
                    let properties = session
                        .as_mut()
                        .expect("package session")
                        .read_text(context, "docProps/app.xml")?
                        .map(|xml| element_text_map(&xml))
                        .unwrap_or_default();
                    for (target, property) in [
                        ("word.page_count", "pages"),
                        ("word.word_count", "words"),
                        ("word.character_count", "characters"),
                        ("word.paragraph_count", "paragraphs"),
                    ] {
                        output.push(numeric_property(target, properties.get(property), path));
                    }
                }
                "word.document_structure" => {
                    let xml = session
                        .as_mut()
                        .expect("package session")
                        .read_text(context, "word/document.xml")?
                        .ok_or_else(|| {
                            DeckProbeError::MalformedInput("missing word/document.xml".to_owned())
                        })?;
                    output.push(Evidence::resolved(
                        "word.paragraph_count",
                        json!(count_start_elements(&xml, "p")),
                        Confidence::Exact,
                        path,
                        "word/document.xml",
                    ));
                    output.push(Evidence::resolved(
                        "word.table_count",
                        json!(count_start_elements(&xml, "tbl")),
                        Confidence::Exact,
                        path,
                        "word/document.xml",
                    ));
                }
                other => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "unknown Word path: {other}"
                    )));
                }
            }
        }
        Ok(output)
    }
}

fn numeric_property(target: &str, value: Option<&String>, path: &str) -> Evidence {
    match value.and_then(|value| value.parse::<u64>().ok()) {
        Some(value) => Evidence::resolved(
            target,
            json!(value),
            Confidence::High,
            path,
            "docProps/app.xml saved statistic",
        ),
        None => Evidence::unresolved(target, deckprobe_core::TargetStatus::Unknown, path),
    }
}
