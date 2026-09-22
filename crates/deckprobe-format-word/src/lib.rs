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
use quick_xml::{Reader, events::Event};
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
                "Last saved word count, or a medium-confidence deep visible-text estimate",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "word.character_count",
                "Last saved character count, or a medium-confidence deep visible-text estimate",
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
            description: "Choose fast saved properties or document XML fallbacks where available"
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
            let may_read_document_xml = Self::stats_path(request) == "auto"
                && request.level >= ProbeLevel::Deep
                && (["word.word_count", "word.character_count"]
                    .iter()
                    .any(|target| {
                        request.targets.contains(*target)
                            && request.minimum_confidence_for(target) <= Confidence::Medium
                    })
                    || request.targets.contains("word.paragraph_count"));
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
                if may_read_document_xml { 48 } else { 8 },
            ));
        }
        if Self::stats_path(request) == "document-xml" {
            paths.push(PathDescriptor::new(
                "word.document_text_statistics",
                &["word.word_count", "word.character_count"],
                ProbeLevel::Deep,
                Confidence::Medium,
                40,
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
        request: &ProbeRequest,
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
                    let wants_document_fallback = Self::stats_path(request) == "auto"
                        && request.level >= ProbeLevel::Deep
                        && [
                            ("word.word_count", "words", Confidence::Medium),
                            ("word.character_count", "characters", Confidence::Medium),
                            ("word.paragraph_count", "paragraphs", Confidence::Exact),
                        ]
                        .iter()
                        .any(|(target, property, confidence)| {
                            request.targets.contains(*target)
                                && request.minimum_confidence_for(target) <= *confidence
                                && !properties.contains_key(*property)
                        });
                    let text_statistics = if wants_document_fallback {
                        let xml = session
                            .as_mut()
                            .expect("package session")
                            .read_text(context, "word/document.xml")?
                            .ok_or_else(|| {
                                DeckProbeError::MalformedInput(
                                    "missing word/document.xml".to_owned(),
                                )
                            })?;
                        Some(document_text_statistics(&xml)?)
                    } else {
                        None
                    };
                    for (target, property) in [
                        ("word.page_count", "pages"),
                        ("word.word_count", "words"),
                        ("word.character_count", "characters"),
                        ("word.paragraph_count", "paragraphs"),
                    ] {
                        let fallback =
                            text_statistics
                                .as_ref()
                                .and_then(|statistics| match target {
                                    "word.word_count" => {
                                        Some((statistics.word_count, Confidence::Medium))
                                    }
                                    "word.character_count" => {
                                        Some((statistics.character_count, Confidence::Medium))
                                    }
                                    "word.paragraph_count" => {
                                        Some((statistics.paragraph_count, Confidence::Exact))
                                    }
                                    _ => None,
                                });
                        output.push(numeric_property(
                            target,
                            properties.get(property),
                            fallback,
                            path,
                        ));
                    }
                }
                "word.document_text_statistics" => {
                    let xml = session
                        .as_mut()
                        .expect("package session")
                        .read_text(context, "word/document.xml")?
                        .ok_or_else(|| {
                            DeckProbeError::MalformedInput("missing word/document.xml".to_owned())
                        })?;
                    let statistics = document_text_statistics(&xml)?;
                    output.extend([
                        Evidence::resolved(
                            "word.word_count",
                            json!(statistics.word_count),
                            Confidence::Medium,
                            path,
                            "estimated from visible text in word/document.xml",
                        ),
                        Evidence::resolved(
                            "word.character_count",
                            json!(statistics.character_count),
                            Confidence::Medium,
                            path,
                            "estimated non-whitespace characters in word/document.xml",
                        ),
                    ]);
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

fn numeric_property(
    target: &str,
    value: Option<&String>,
    fallback: Option<(u64, Confidence)>,
    path: &str,
) -> Evidence {
    match value {
        Some(value) => match value.parse::<u64>() {
            Ok(value) => Evidence::resolved(
                target,
                json!(value),
                Confidence::High,
                path,
                "docProps/app.xml saved statistic",
            ),
            Err(_) => {
                let mut evidence =
                    Evidence::unresolved(target, deckprobe_core::TargetStatus::Invalid, path);
                evidence.source = "docProps/app.xml contains an invalid saved statistic".to_owned();
                evidence
            }
        },
        None if fallback.is_some() => {
            let (value, confidence) = fallback.expect("checked above");
            let source = match target {
                "word.word_count" => "estimated from visible text in word/document.xml",
                "word.character_count" => {
                    "estimated non-whitespace characters in word/document.xml"
                }
                "word.paragraph_count" => "paragraph elements in word/document.xml",
                _ => "word/document.xml fallback",
            };
            Evidence::resolved(target, json!(value), confidence, path, source)
        }
        None => Evidence::resolved(
            target,
            serde_json::Value::Null,
            Confidence::High,
            path,
            "optional saved statistic is absent from docProps/app.xml",
        ),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DocumentTextStatistics {
    word_count: u64,
    character_count: u64,
    paragraph_count: u64,
}

fn document_text_statistics(xml: &str) -> Result<DocumentTextStatistics> {
    let mut reader = Reader::from_str(xml);
    let mut statistics = DocumentTextStatistics::default();
    let mut paragraph_text = String::new();
    let mut text_depth = 0_u32;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match xml_local_name(event.name().as_ref()) {
                b"p" => {
                    paragraph_text.clear();
                    statistics.paragraph_count += 1;
                }
                b"t" => text_depth += 1,
                _ => {}
            },
            Ok(Event::Empty(event)) if xml_local_name(event.name().as_ref()) == b"p" => {
                statistics.paragraph_count += 1;
            }
            Ok(Event::Text(event)) if text_depth > 0 => {
                let decoded = event.decode().map_err(|error| {
                    DeckProbeError::MalformedInput(format!(
                        "word/document.xml contains invalid text: {error}"
                    ))
                })?;
                let text = quick_xml::escape::unescape(&decoded).map_err(|error| {
                    DeckProbeError::MalformedInput(format!(
                        "word/document.xml contains invalid escaped text: {error}"
                    ))
                })?;
                statistics.character_count += text
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .count() as u64;
                paragraph_text.push_str(&text);
            }
            Ok(Event::End(event)) => match xml_local_name(event.name().as_ref()) {
                b"t" => text_depth = text_depth.saturating_sub(1),
                b"p" => statistics.word_count += estimated_word_count(&paragraph_text),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(DeckProbeError::MalformedInput(format!(
                    "word/document.xml is not valid XML: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(statistics)
}

fn xml_local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|value| *value == b':').next().unwrap_or(name)
}

fn estimated_word_count(text: &str) -> u64 {
    let mut count = 0;
    let mut in_word = false;
    for character in text.chars() {
        if is_east_asian_word_character(character) {
            if in_word {
                count += 1;
                in_word = false;
            }
            count += 1;
        } else if character.is_alphanumeric() {
            in_word = true;
        } else if in_word {
            count += 1;
            in_word = false;
        }
    }
    count + u64::from(in_word)
}

fn is_east_asian_word_character(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0x3040..=0x30ff
            | 0xac00..=0xd7af
            | 0xf900..=0xfaff
            | 0x20000..=0x2ffff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_text_statistics_join_runs_and_handle_east_asian_text() {
        let xml = r#"<w:document xmlns:w="urn:test"><w:body>
            <w:p><w:r><w:t>Hello </w:t></w:r><w:r><w:t>world</w:t></w:r></w:p>
            <w:p><w:r><w:t>台灣 AI</w:t></w:r></w:p>
        </w:body></w:document>"#;
        assert_eq!(
            document_text_statistics(xml).unwrap(),
            DocumentTextStatistics {
                word_count: 5,
                character_count: 14,
                paragraph_count: 2,
            }
        );
    }

    #[test]
    fn missing_saved_statistic_is_a_resolved_null() {
        let evidence = numeric_property("word.page_count", None, None, "word.app_statistics");
        assert_eq!(evidence.status, deckprobe_core::TargetStatus::Resolved);
        assert_eq!(evidence.value, Some(serde_json::Value::Null));
    }
}
