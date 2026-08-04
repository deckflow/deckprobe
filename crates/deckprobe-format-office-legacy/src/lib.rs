use std::collections::{BTreeMap, BTreeSet};

use deckprobe_core::{
    Confidence, DeckProbeError, Evidence, ExecutionPlan, FormatDriver, FormatProfile, OptionSpec,
    PathDescriptor, ProbeContext, ProbeLevel, ProbeRequest, Result, TargetScope, TargetSpec,
    TargetStatus, common_target_specs,
};
use office_oxide::cfb::CfbReader;
use office_oxide::doc::DocDocument;
use office_oxide::ppt::{PptDocument, TextType};
use office_oxide::xls::XlsDocument;
use serde_json::json;

pub const LEGACY_DOC: FormatProfile = FormatProfile {
    driver: "office-legacy",
    format: "office-legacy",
    profile: "doc",
    mime_type: "application/msword",
    extensions: &["doc", "dot"],
};
pub const LEGACY_XLS: FormatProfile = FormatProfile {
    driver: "office-legacy",
    format: "office-legacy",
    profile: "xls",
    mime_type: "application/vnd.ms-excel",
    extensions: &["xls", "xlt"],
};
pub const LEGACY_PPT: FormatProfile = FormatProfile {
    driver: "office-legacy",
    format: "office-legacy",
    profile: "ppt",
    mime_type: "application/vnd.ms-powerpoint",
    extensions: &["ppt", "pps", "pot"],
};
pub const ENCRYPTED_OOXML: FormatProfile = FormatProfile {
    driver: "office-legacy",
    format: "office-open-xml",
    profile: "encrypted-ooxml",
    mime_type: "application/x-ole-storage",
    extensions: &[
        "docx", "docm", "dotx", "dotm", "xlsx", "xlsm", "xltx", "xltm", "xlsb", "pptx", "pptm",
        "ppsx", "ppsm", "potx", "potm",
    ],
};

pub fn profile_for_extension(extension: Option<&str>) -> FormatProfile {
    match extension.unwrap_or_default() {
        "doc" | "dot" => LEGACY_DOC,
        "xls" | "xlt" => LEGACY_XLS,
        "ppt" | "pps" | "pot" => LEGACY_PPT,
        "docx" | "docm" | "dotx" | "dotm" | "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" | "pptx"
        | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" => ENCRYPTED_OOXML,
        _ => LEGACY_DOC,
    }
}

pub struct OfficeLegacyDriver {
    profile: FormatProfile,
}

impl OfficeLegacyDriver {
    pub fn new(profile: FormatProfile) -> Self {
        Self { profile }
    }

    fn profile_targets(&self) -> Vec<TargetSpec> {
        use ProbeLevel::Metadata;
        use TargetScope::Format;
        match self.profile.profile {
            "doc" => vec![
                TargetSpec::new(
                    "word.page_count",
                    "Last saved page count",
                    "u64|null",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "word.word_count",
                    "Extracted word count",
                    "u64|null",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "word.character_count",
                    "Extracted character count",
                    "u64|null",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "word.paragraph_count",
                    "Extracted paragraph count",
                    "u64|null",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "word.is_template",
                    "Whether the suffix is a Word template",
                    "bool",
                    Format,
                    Metadata,
                ),
            ],
            "xls" => vec![
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
                    "excel.is_template",
                    "Whether the suffix is an Excel template",
                    "bool",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "excel.binary_workbook",
                    "Whether the workbook uses a binary main stream",
                    "bool",
                    Format,
                    Metadata,
                ),
            ],
            "ppt" => vec![
                TargetSpec::new(
                    "powerpoint.slide_count",
                    "Logical slide count",
                    "u64|null",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "powerpoint.notes_slide_count",
                    "Slides with speaker notes",
                    "u64|null",
                    Format,
                    Metadata,
                ),
                TargetSpec::new(
                    "powerpoint.presentation_kind",
                    "presentation/show/template",
                    "string",
                    Format,
                    Metadata,
                ),
            ],
            _ => Vec::new(),
        }
    }

    fn statistic_targets(&self) -> &'static [&'static str] {
        match self.profile.profile {
            "doc" => &[
                "word.page_count",
                "word.word_count",
                "word.character_count",
                "word.paragraph_count",
                "word.is_template",
            ],
            "xls" => &[
                "excel.sheet_count",
                "excel.sheet_names",
                "excel.is_template",
                "excel.binary_workbook",
            ],
            "ppt" => &[
                "powerpoint.slide_count",
                "powerpoint.notes_slide_count",
                "powerpoint.presentation_kind",
            ],
            _ => &[],
        }
    }
}

impl FormatDriver for OfficeLegacyDriver {
    fn id(&self) -> &'static str {
        "office-legacy"
    }

    fn profile(&self) -> &FormatProfile {
        &self.profile
    }

    fn targets(&self) -> Vec<TargetSpec> {
        let mut targets = common_target_specs();
        targets.extend([
            TargetSpec::new(
                "office.document_kind",
                "Office family: word/excel/powerpoint",
                "string",
                TargetScope::Office,
                ProbeLevel::Header,
            ),
            TargetSpec::new(
                "office.legacy_kind",
                "Validated legacy Word/Excel/PowerPoint kind",
                "string",
                TargetScope::Format,
                ProbeLevel::Header,
            ),
            TargetSpec::new(
                "office.cfb_container",
                "Compound File Binary container detected",
                "bool",
                TargetScope::Format,
                ProbeLevel::Header,
            ),
            TargetSpec::new(
                "office.cfb_entry_count",
                "CFB directory entry count",
                "u64",
                TargetScope::Format,
                ProbeLevel::Metadata,
            ),
            TargetSpec::new(
                "office.content_probe_supported",
                "Binary content probing available",
                "bool",
                TargetScope::Format,
                ProbeLevel::Header,
            ),
        ]);
        targets.extend(self.profile_targets());
        targets
    }

    fn options(&self) -> Vec<OptionSpec> {
        Vec::new()
    }

    fn default_targets(&self, level: ProbeLevel) -> BTreeSet<String> {
        let mut targets = [
            "document.format",
            "document.format_profile",
            "document.mime_type",
            "document.file_size",
            "document.extension",
            "document.extension_matches",
            "security.encrypted",
            "office.document_kind",
            "office.legacy_kind",
            "office.cfb_container",
            "office.content_probe_supported",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
        if level >= ProbeLevel::Metadata {
            targets.extend(
                [
                    "document.title",
                    "document.author",
                    "document.application",
                    "security.has_macros",
                    "office.cfb_entry_count",
                ]
                .into_iter()
                .map(str::to_owned),
            );
            targets.extend(
                self.statistic_targets()
                    .iter()
                    .map(|value| (*value).to_owned()),
            );
        }
        targets
    }

    fn paths(&self, request: &ProbeRequest) -> Result<Vec<PathDescriptor>> {
        self.validate_options(request)?;
        Ok(vec![
            PathDescriptor::new(
                "office_legacy.cfb_inventory",
                &[
                    "document.format",
                    "document.format_profile",
                    "document.mime_type",
                    "document.file_size",
                    "document.extension",
                    "document.extension_matches",
                    "security.encrypted",
                    "security.has_macros",
                    "security.password_protected",
                    "security.has_embedded_files",
                    "office.document_kind",
                    "office.legacy_kind",
                    "office.cfb_container",
                    "office.cfb_entry_count",
                    "office.content_probe_supported",
                ],
                ProbeLevel::Header,
                Confidence::Exact,
                4,
            ),
            PathDescriptor::new(
                "office_legacy.summary_information",
                &[
                    "document.title",
                    "document.subject",
                    "document.author",
                    "document.keywords",
                    "document.created_at",
                    "document.modified_at",
                    "document.application",
                    "word.page_count",
                    "word.word_count",
                    "word.character_count",
                ],
                ProbeLevel::Metadata,
                Confidence::High,
                8,
            ),
            PathDescriptor::new(
                "office_legacy.content_statistics",
                self.statistic_targets(),
                ProbeLevel::Metadata,
                Confidence::Exact,
                50,
            ),
        ])
    }

    fn validate_options(&self, request: &ProbeRequest) -> Result<()> {
        if let Some((key, value)) = request.format_options.iter().next() {
            return Err(DeckProbeError::InvalidRequest(format!(
                "unknown legacy option {key}={value}"
            )));
        }
        Ok(())
    }

    fn execute(
        &self,
        context: &mut ProbeContext,
        _request: &ProbeRequest,
        plan: &ExecutionPlan,
    ) -> Result<Vec<Evidence>> {
        let mut output = Vec::new();
        let needs_cfb = plan.paths.iter().any(|path| {
            matches!(
                path.as_str(),
                "office_legacy.cfb_inventory" | "office_legacy.summary_information"
            )
        });
        let mut cfb = if needs_cfb {
            Some(
                CfbReader::new(context.open_budgeted_reader()?).map_err(|error| {
                    map_legacy_error(context, "cannot parse CFB directory", error)
                })?,
            )
        } else {
            None
        };
        if let Some(cfb) = cfb.as_ref() {
            validate_kind(cfb, self.profile.profile)?;
        }

        for path in &plan.paths {
            context.check_time()?;
            match path.as_str() {
                "office_legacy.cfb_inventory" => {
                    let cfb = cfb.as_ref().expect("CFB session");
                    let kind = detected_kind(cfb).expect("validated CFB kind");
                    let encrypted = kind == "encrypted-ooxml";
                    let macros = cfb.entries().iter().any(|entry| {
                        let name = entry.name.to_ascii_lowercase();
                        name.contains("vba") || name == "macros"
                    });
                    let embedded_files = cfb.entries().iter().any(|entry| {
                        let name = entry.name.to_ascii_lowercase();
                        name.contains("objectpool")
                            || name.contains("ole10native")
                            || name.contains("package")
                    });
                    output.extend(deckprobe_core::identity_evidence(
                        context,
                        &self.profile,
                        path,
                    ));
                    output.extend([
                        if encrypted {
                            Evidence::resolved(
                                "security.encrypted",
                                true,
                                Confidence::Exact,
                                path,
                                "EncryptionInfo + EncryptedPackage streams",
                            )
                        } else {
                            Evidence::unresolved("security.encrypted", TargetStatus::Unknown, path)
                        },
                        if encrypted {
                            Evidence::resolved(
                                "security.password_protected",
                                true,
                                Confidence::Exact,
                                path,
                                "EncryptionInfo + EncryptedPackage streams",
                            )
                        } else {
                            Evidence::unresolved(
                                "security.password_protected",
                                TargetStatus::Unknown,
                                path,
                            )
                        },
                        Evidence::resolved(
                            "security.has_macros",
                            macros,
                            Confidence::Exact,
                            path,
                            "CFB directory inventory",
                        ),
                        Evidence::resolved(
                            "security.has_embedded_files",
                            embedded_files,
                            Confidence::Exact,
                            path,
                            "CFB embedded-object storage inventory",
                        ),
                        Evidence::resolved(
                            "office.document_kind",
                            office_kind(kind),
                            Confidence::Exact,
                            path,
                            "validated CFB main stream",
                        ),
                        Evidence::resolved(
                            "office.legacy_kind",
                            kind,
                            Confidence::Exact,
                            path,
                            "validated CFB main stream",
                        ),
                        Evidence::resolved(
                            "office.cfb_container",
                            true,
                            Confidence::Exact,
                            path,
                            "CFB header and directory",
                        ),
                        Evidence::resolved(
                            "office.cfb_entry_count",
                            json!(cfb.entries().len()),
                            Confidence::Exact,
                            path,
                            "CFB directory",
                        ),
                        Evidence::resolved(
                            "office.content_probe_supported",
                            !encrypted,
                            Confidence::Exact,
                            path,
                            "legacy parser capability",
                        ),
                    ]);
                }
                "office_legacy.summary_information" => {
                    let cfb = cfb.as_mut().expect("CFB session");
                    let summary = cfb
                        .open_stream("\u{5}SummaryInformation")
                        .ok()
                        .map(|bytes| parse_summary_information(&bytes))
                        .unwrap_or_default();
                    output.extend(summary_evidence(&summary, path, self.profile.profile));
                }
                "office_legacy.content_statistics" => {
                    output.extend(content_statistics(context, &self.profile, path)?);
                }
                other => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "unknown legacy path: {other}"
                    )));
                }
            }
            context.check_time()?;
        }
        Ok(output)
    }
}

fn detected_kind<R: std::io::Read + std::io::Seek>(cfb: &CfbReader<R>) -> Option<&'static str> {
    if cfb.has_stream("EncryptionInfo") && cfb.has_stream("EncryptedPackage") {
        Some("encrypted-ooxml")
    } else if cfb.has_stream("WordDocument") {
        Some("doc")
    } else if cfb.has_stream("Workbook") || cfb.has_stream("Book") {
        Some("xls")
    } else if cfb.has_stream("PowerPoint Document") || cfb.has_stream("PP97_DUALSTORAGE") {
        Some("ppt")
    } else {
        None
    }
}

fn validate_kind<R: std::io::Read + std::io::Seek>(
    cfb: &CfbReader<R>,
    expected: &str,
) -> Result<()> {
    let actual = detected_kind(cfb).ok_or_else(|| {
        DeckProbeError::MalformedInput(
            "CFB container has no supported Office main stream".to_owned(),
        )
    })?;
    if actual != expected {
        return Err(DeckProbeError::MalformedInput(format!(
            ".{expected} suffix does not match CFB {actual} content"
        )));
    }
    Ok(())
}

fn office_kind(kind: &str) -> &str {
    match kind {
        "doc" => "word",
        "xls" => "excel",
        "ppt" => "powerpoint",
        _ => "encrypted",
    }
}

fn content_statistics(
    context: &ProbeContext,
    profile: &FormatProfile,
    path: &str,
) -> Result<Vec<Evidence>> {
    let reader = context.open_budgeted_reader()?;
    let evidence = match profile.profile {
        "doc" => {
            let document = DocDocument::from_reader(reader)
                .map_err(|error| map_legacy_error(context, "cannot parse DOC content", error))?;
            let text = document.plain_text_ref();
            vec![
                Evidence::resolved(
                    "word.word_count",
                    json!(text.split_whitespace().count()),
                    Confidence::Exact,
                    path,
                    "DOC piece table",
                ),
                Evidence::resolved(
                    "word.character_count",
                    json!(text.chars().count()),
                    Confidence::Exact,
                    path,
                    "DOC piece table",
                ),
                Evidence::resolved(
                    "word.paragraph_count",
                    json!(text.lines().filter(|line| !line.trim().is_empty()).count()),
                    Confidence::Exact,
                    path,
                    "DOC piece table",
                ),
                Evidence::resolved(
                    "word.is_template",
                    context.extension().as_deref() == Some("dot"),
                    Confidence::Exact,
                    path,
                    "input suffix",
                ),
            ]
        }
        "xls" => {
            let document = XlsDocument::from_reader(reader)
                .map_err(|error| map_legacy_error(context, "cannot parse XLS content", error))?;
            let names = document
                .sheets
                .iter()
                .map(|sheet| sheet.name.clone())
                .collect::<Vec<_>>();
            vec![
                Evidence::resolved(
                    "excel.sheet_count",
                    json!(document.sheets.len()),
                    Confidence::Exact,
                    path,
                    "BIFF workbook stream",
                ),
                Evidence::resolved(
                    "excel.sheet_names",
                    json!(names),
                    Confidence::Exact,
                    path,
                    "BIFF workbook stream",
                ),
                Evidence::resolved(
                    "excel.is_template",
                    context.extension().as_deref() == Some("xlt"),
                    Confidence::Exact,
                    path,
                    "input suffix",
                ),
                Evidence::resolved(
                    "excel.binary_workbook",
                    true,
                    Confidence::Exact,
                    path,
                    "BIFF workbook stream",
                ),
            ]
        }
        "ppt" => {
            let document = PptDocument::from_reader(reader)
                .map_err(|error| map_legacy_error(context, "cannot parse PPT content", error))?;
            let notes = document
                .slides
                .iter()
                .filter(|slide| {
                    slide
                        .text_runs
                        .iter()
                        .any(|run| run.text_type == TextType::Notes)
                })
                .count();
            let presentation_kind = match context.extension().as_deref() {
                Some("pps") => "show",
                Some("pot") => "template",
                _ => "presentation",
            };
            vec![
                Evidence::resolved(
                    "powerpoint.slide_count",
                    json!(document.slides.len()),
                    Confidence::Exact,
                    path,
                    "PPT persist directory",
                ),
                Evidence::resolved(
                    "powerpoint.notes_slide_count",
                    json!(notes),
                    Confidence::Exact,
                    path,
                    "PPT text records",
                ),
                Evidence::resolved(
                    "powerpoint.presentation_kind",
                    presentation_kind,
                    Confidence::Exact,
                    path,
                    "input suffix",
                ),
            ]
        }
        _ => Vec::new(),
    };
    context.check_time()?;
    Ok(evidence)
}

#[derive(Debug, Clone)]
enum SummaryValue {
    Text(String),
    Number(u64),
}

fn parse_summary_information(bytes: &[u8]) -> BTreeMap<u32, SummaryValue> {
    let mut values = BTreeMap::new();
    if bytes.len() < 48 || bytes.get(0..2) != Some(&[0xfe, 0xff]) {
        return values;
    }
    let Some(section_offset) = read_u32(bytes, 44).map(|value| value as usize) else {
        return values;
    };
    let Some(property_count) = read_u32(bytes, section_offset + 4).map(|value| value as usize)
    else {
        return values;
    };
    for index in 0..property_count.min(256) {
        let entry = section_offset + 8 + index * 8;
        let Some(id) = read_u32(bytes, entry) else {
            break;
        };
        let Some(offset) = read_u32(bytes, entry + 4).map(|value| value as usize) else {
            break;
        };
        let value_offset = section_offset.saturating_add(offset);
        let Some(value_type) = read_u32(bytes, value_offset) else {
            continue;
        };
        let value = match value_type {
            2 => read_u16(bytes, value_offset + 4).map(|value| SummaryValue::Number(value as u64)),
            3 => read_u32(bytes, value_offset + 4).map(|value| SummaryValue::Number(value as u64)),
            30 => read_u32(bytes, value_offset + 4).and_then(|length| {
                let start = value_offset + 8;
                let end = start.checked_add(length as usize)?.min(bytes.len());
                (start <= end).then(|| {
                    let raw = &bytes[start..end];
                    SummaryValue::Text(
                        String::from_utf8_lossy(raw)
                            .trim_end_matches('\0')
                            .to_owned(),
                    )
                })
            }),
            31 => read_u32(bytes, value_offset + 4).and_then(|length| {
                let start = value_offset + 8;
                let end = start.checked_add(length as usize * 2)?.min(bytes.len());
                let words = bytes
                    .get(start..end)?
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .take_while(|value| *value != 0)
                    .collect::<Vec<_>>();
                Some(SummaryValue::Text(String::from_utf16_lossy(&words)))
            }),
            _ => None,
        };
        if let Some(value) = value {
            values.insert(id, value);
        }
    }
    values
}

fn summary_evidence(
    summary: &BTreeMap<u32, SummaryValue>,
    path: &str,
    profile: &str,
) -> Vec<Evidence> {
    let mut output = Vec::new();
    for (target, id) in [
        ("document.title", 2),
        ("document.subject", 3),
        ("document.author", 4),
        ("document.keywords", 5),
        ("document.application", 18),
    ] {
        output.push(match summary.get(&id) {
            Some(SummaryValue::Text(value)) if !value.is_empty() => Evidence::resolved(
                target,
                value.clone(),
                Confidence::High,
                path,
                "SummaryInformation property set",
            ),
            _ => Evidence::unresolved(target, TargetStatus::Unknown, path),
        });
    }
    for target in ["document.created_at", "document.modified_at"] {
        output.push(Evidence::unresolved(target, TargetStatus::Unknown, path));
    }
    if profile == "doc" {
        for (target, id) in [
            ("word.page_count", 14),
            ("word.word_count", 15),
            ("word.character_count", 16),
        ] {
            if let Some(SummaryValue::Number(value)) = summary.get(&id) {
                output.push(Evidence::resolved(
                    target,
                    json!(value),
                    Confidence::High,
                    path,
                    "SummaryInformation saved statistic",
                ));
            }
        }
    }
    output
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn map_legacy_error(
    context: &ProbeContext,
    prefix: &str,
    error: impl std::fmt::Display,
) -> DeckProbeError {
    let message = error.to_string();
    if context.check_time().is_err() || message.contains("deckprobe") {
        DeckProbeError::BudgetExceeded(message)
    } else {
        DeckProbeError::MalformedInput(format!("{prefix}: {message}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_parser_reads_strings_and_numbers() {
        let mut bytes = vec![0_u8; 128];
        bytes[0..2].copy_from_slice(&[0xfe, 0xff]);
        bytes[44..48].copy_from_slice(&48_u32.to_le_bytes());
        bytes[52..56].copy_from_slice(&2_u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&2_u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&24_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(&15_u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&40_u32.to_le_bytes());
        bytes[72..76].copy_from_slice(&31_u32.to_le_bytes());
        bytes[76..80].copy_from_slice(&4_u32.to_le_bytes());
        bytes[80..88].copy_from_slice(&[b'T', 0, b'e', 0, b's', 0, b't', 0]);
        bytes[88..92].copy_from_slice(&3_u32.to_le_bytes());
        bytes[92..96].copy_from_slice(&42_u32.to_le_bytes());
        let parsed = parse_summary_information(&bytes);
        assert!(matches!(parsed.get(&2), Some(SummaryValue::Text(value)) if value == "Test"));
        assert!(matches!(parsed.get(&15), Some(SummaryValue::Number(42))));
    }
}
