use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Seek};

use deckprobe_core::{
    Confidence, DeckProbeError, Evidence, FormatProfile, ProbeContext, Result, TargetScope,
    TargetSpec,
};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::json;
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OoxmlFamily {
    Word,
    Excel,
    PowerPoint,
}

#[derive(Debug, Clone)]
pub struct OoxmlDetection {
    pub family: OoxmlFamily,
    pub profile: FormatProfile,
}

pub const DOCX: FormatProfile = FormatProfile {
    driver: "word",
    format: "office-open-xml",
    profile: "docx",
    mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    extensions: &["docx"],
};
pub const DOCM: FormatProfile = FormatProfile {
    driver: "word",
    format: "office-open-xml",
    profile: "docm",
    mime_type: "application/vnd.ms-word.document.macroenabled.12",
    extensions: &["docm"],
};
pub const DOTX: FormatProfile = FormatProfile {
    driver: "word",
    format: "office-open-xml",
    profile: "dotx",
    mime_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
    extensions: &["dotx"],
};
pub const DOTM: FormatProfile = FormatProfile {
    driver: "word",
    format: "office-open-xml",
    profile: "dotm",
    mime_type: "application/vnd.ms-word.template.macroenabled.12",
    extensions: &["dotm"],
};
pub const XLSX: FormatProfile = FormatProfile {
    driver: "excel",
    format: "office-open-xml",
    profile: "xlsx",
    mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    extensions: &["xlsx"],
};
pub const XLSM: FormatProfile = FormatProfile {
    driver: "excel",
    format: "office-open-xml",
    profile: "xlsm",
    mime_type: "application/vnd.ms-excel.sheet.macroenabled.12",
    extensions: &["xlsm"],
};
pub const XLTX: FormatProfile = FormatProfile {
    driver: "excel",
    format: "office-open-xml",
    profile: "xltx",
    mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
    extensions: &["xltx"],
};
pub const XLTM: FormatProfile = FormatProfile {
    driver: "excel",
    format: "office-open-xml",
    profile: "xltm",
    mime_type: "application/vnd.ms-excel.template.macroenabled.12",
    extensions: &["xltm"],
};
pub const XLSB: FormatProfile = FormatProfile {
    driver: "excel",
    format: "office-open-xml",
    profile: "xlsb",
    mime_type: "application/vnd.ms-excel.sheet.binary.macroenabled.12",
    extensions: &["xlsb"],
};
pub const PPTX: FormatProfile = FormatProfile {
    driver: "powerpoint",
    format: "office-open-xml",
    profile: "pptx",
    mime_type: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    extensions: &["pptx"],
};
pub const PPTM: FormatProfile = FormatProfile {
    driver: "powerpoint",
    format: "office-open-xml",
    profile: "pptm",
    mime_type: "application/vnd.ms-powerpoint.presentation.macroenabled.12",
    extensions: &["pptm"],
};
pub const PPSX: FormatProfile = FormatProfile {
    driver: "powerpoint",
    format: "office-open-xml",
    profile: "ppsx",
    mime_type: "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
    extensions: &["ppsx"],
};
pub const PPSM: FormatProfile = FormatProfile {
    driver: "powerpoint",
    format: "office-open-xml",
    profile: "ppsm",
    mime_type: "application/vnd.ms-powerpoint.slideshow.macroenabled.12",
    extensions: &["ppsm"],
};
pub const POTX: FormatProfile = FormatProfile {
    driver: "powerpoint",
    format: "office-open-xml",
    profile: "potx",
    mime_type: "application/vnd.openxmlformats-officedocument.presentationml.template",
    extensions: &["potx"],
};
pub const POTM: FormatProfile = FormatProfile {
    driver: "powerpoint",
    format: "office-open-xml",
    profile: "potm",
    mime_type: "application/vnd.ms-powerpoint.template.macroenabled.12",
    extensions: &["potm"],
};

pub fn detect(context: &ProbeContext) -> Result<OoxmlDetection> {
    let reader = context.open_budgeted_reader()?;
    let mut archive = ZipArchive::new(reader).map_err(|error| {
        DeckProbeError::MalformedInput(format!("invalid ZIP/OPC container: {error}"))
    })?;
    let mut content_types = String::new();
    archive
        .by_name("[Content_Types].xml")
        .map_err(|_| DeckProbeError::UnsupportedFormat("ZIP is not an OOXML package".to_owned()))?
        .take(2 * 1024 * 1024)
        .read_to_string(&mut content_types)?;
    let lower = content_types.to_ascii_lowercase();
    let extension = context.extension().unwrap_or_default();
    let macro_enabled =
        lower.contains("macroenabled") || has_entry_suffix(&mut archive, "vbaproject.bin");

    // Anchor the family on the package main part. Arbitrary content-type
    // substrings are unsafe because a presentation can embed a workbook.
    let detection = if has_entry(&mut archive, "word/document.xml") {
        let profile = match (extension.as_str(), macro_enabled) {
            ("dotx", false) => DOTX,
            ("dotm", _) => DOTM,
            (_, true) => DOCM,
            _ => DOCX,
        };
        OoxmlDetection {
            family: OoxmlFamily::Word,
            profile,
        }
    } else if has_entry(&mut archive, "ppt/presentation.xml") {
        let profile = match (extension.as_str(), macro_enabled) {
            ("ppsx", false) => PPSX,
            ("ppsm", _) => PPSM,
            ("potx", false) => POTX,
            ("potm", _) => POTM,
            (_, true) => PPTM,
            _ => PPTX,
        };
        OoxmlDetection {
            family: OoxmlFamily::PowerPoint,
            profile,
        }
    } else if has_entry(&mut archive, "xl/workbook.xml")
        || has_entry(&mut archive, "xl/workbook.bin")
    {
        let profile = match extension.as_str() {
            "xlsb" => XLSB,
            "xltx" if !macro_enabled => XLTX,
            "xltm" => XLTM,
            _ if macro_enabled => XLSM,
            _ => XLSX,
        };
        OoxmlDetection {
            family: OoxmlFamily::Excel,
            profile,
        }
    } else {
        return Err(DeckProbeError::UnsupportedFormat(
            "ZIP is not a supported Word/Excel/PowerPoint package".to_owned(),
        ));
    };
    Ok(detection)
}

fn has_entry<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> bool {
    archive.by_name(name).is_ok()
}

fn has_entry_suffix<R: Read + Seek>(archive: &mut ZipArchive<R>, suffix: &str) -> bool {
    let suffix = suffix.to_ascii_lowercase();
    (0..archive.len()).any(|index| {
        archive
            .by_index(index)
            .map(|entry| entry.name().to_ascii_lowercase().ends_with(&suffix))
            .unwrap_or(false)
    })
}

pub struct OoxmlSession<R: Read + Seek> {
    archive: ZipArchive<R>,
    entry_names: Vec<String>,
    text_cache: HashMap<String, String>,
    content_types_cache: Option<PartContentTypes>,
    security_cache: Option<OoxmlSecurityInventory>,
}

#[derive(Debug, Clone, Default)]
struct PartContentTypes {
    defaults: BTreeMap<String, String>,
    overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct OoxmlSecurityInventory {
    has_macros: bool,
    signature_count: usize,
    has_signature_structure: bool,
    has_external_relationships: bool,
    has_embedded_files: bool,
}

impl OoxmlSession<deckprobe_core::BudgetedReader<deckprobe_core::BoxedProbeReader>> {
    pub fn open(context: &ProbeContext) -> Result<Self> {
        let reader = context.open_budgeted_reader()?;
        let archive = ZipArchive::new(reader)
            .map_err(|error| map_zip_error(context, "invalid ZIP/OPC container", error))?;
        if archive.len() > context.budget().max_archive_entries {
            return Err(DeckProbeError::BudgetExceeded(format!(
                "archive entries {} exceed budget {}",
                archive.len(),
                context.budget().max_archive_entries
            )));
        }
        let entry_names = archive.file_names().map(str::to_owned).collect();
        context.check_time()?;
        Ok(Self {
            archive,
            entry_names,
            text_cache: HashMap::new(),
            content_types_cache: None,
            security_cache: None,
        })
    }
}

impl<R: Read + Seek> OoxmlSession<R> {
    pub fn entry_count(&self) -> usize {
        self.entry_names.len()
    }

    pub fn entry_names(&self) -> &[String] {
        &self.entry_names
    }

    pub fn has_entry(&self, name: &str) -> bool {
        self.entry_names.iter().any(|value| value == name)
    }

    pub fn has_suffix(&self, suffix: &str) -> bool {
        let suffix = suffix.to_ascii_lowercase();
        self.entry_names
            .iter()
            .any(|value| value.to_ascii_lowercase().ends_with(&suffix))
    }

    pub fn validate_profile(
        &mut self,
        context: &ProbeContext,
        profile: &FormatProfile,
    ) -> Result<()> {
        let main_part = match profile.driver {
            "word" => "word/document.xml",
            "excel" if profile.profile == "xlsb" => "xl/workbook.bin",
            "excel" => "xl/workbook.xml",
            "powerpoint" => "ppt/presentation.xml",
            _ => {
                return Err(DeckProbeError::InvalidRequest(format!(
                    "unsupported OOXML profile {}",
                    profile.profile
                )));
            }
        };
        if !self.has_entry(main_part) {
            return Err(DeckProbeError::MalformedInput(format!(
                ".{} package is missing required main part {main_part}",
                profile.profile
            )));
        }
        let content_types = self
            .read_text(context, "[Content_Types].xml")?
            .ok_or_else(|| {
                DeckProbeError::MalformedInput("missing [Content_Types].xml".to_owned())
            })?;
        if !content_types
            .to_ascii_lowercase()
            .contains(&profile.mime_type.to_ascii_lowercase())
        {
            return Err(DeckProbeError::MalformedInput(format!(
                "package main content type does not match .{}",
                profile.profile
            )));
        }
        context.check_time()
    }

    pub fn read_text(&mut self, context: &ProbeContext, name: &str) -> Result<Option<String>> {
        if let Some(value) = self.text_cache.get(name) {
            return Ok(Some(value.clone()));
        }
        let Ok(mut entry) = self.archive.by_name(name) else {
            return Ok(None);
        };
        let declared_size = entry.size();
        context.record_expanded(declared_size)?;
        if declared_size > context.budget().max_expanded_bytes {
            return Err(DeckProbeError::BudgetExceeded(format!(
                "entry {name} size {declared_size} exceeds expanded budget"
            )));
        }
        let mut value = String::with_capacity(declared_size.min(1024 * 1024) as usize);
        entry.read_to_string(&mut value).map_err(|error| {
            if let Some(message) = context.budget_failure() {
                DeckProbeError::BudgetExceeded(message)
            } else if error.to_string().contains("deckprobe") {
                DeckProbeError::BudgetExceeded(error.to_string())
            } else {
                DeckProbeError::MalformedInput(format!("{name} is not valid UTF-8 XML: {error}"))
            }
        })?;
        context.check_time()?;
        self.text_cache.insert(name.to_owned(), value.clone());
        Ok(Some(value))
    }

    fn security_inventory(&mut self, context: &ProbeContext) -> Result<OoxmlSecurityInventory> {
        if let Some(inventory) = &self.security_cache {
            return Ok(inventory.clone());
        }

        let mut facts = OoxmlSecurityInventory {
            has_macros: self.has_suffix("vbaproject.bin"),
            signature_count: self.signature_part_names(context)?.len(),
            has_signature_structure: self
                .entry_names
                .iter()
                .any(|name| name.to_ascii_lowercase().starts_with("_xmlsignatures/")),
            has_external_relationships: false,
            has_embedded_files: self.entry_names.iter().any(|name| {
                name.to_ascii_lowercase()
                    .split('/')
                    .any(|segment| segment == "embeddings")
            }),
        };
        facts.has_signature_structure |= facts.signature_count > 0;

        let relationship_parts = self
            .entry_names
            .iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(".rels"))
            .cloned()
            .collect::<Vec<_>>();
        for name in relationship_parts {
            context.check_time()?;
            let Some(xml) = self.read_text(context, &name)? else {
                continue;
            };
            let relationship_facts = parse_relationship_facts(&xml, &name)?;
            facts.has_external_relationships |= relationship_facts.has_external;
            facts.has_embedded_files |= relationship_facts.has_embedded;
            facts.has_signature_structure |= relationship_facts.has_signature;
        }

        self.security_cache = Some(facts.clone());
        Ok(facts)
    }

    fn signature_part_names(&mut self, context: &ProbeContext) -> Result<BTreeSet<String>> {
        let mut parts = self
            .entry_names
            .iter()
            .filter_map(|name| {
                let lower = name.to_ascii_lowercase();
                let standard_signature_name = lower
                    .rsplit('/')
                    .next()
                    .is_some_and(|file_name| file_name.starts_with("sig"));
                (standard_signature_name
                    && lower.starts_with("_xmlsignatures/")
                    && lower.ends_with(".xml")
                    && !lower.contains("/_rels/"))
                .then_some(lower)
            })
            .collect::<BTreeSet<_>>();

        if let Some(xml) = self.read_text(context, "[Content_Types].xml")? {
            for part_name in digital_signature_overrides(&xml)? {
                let normalized = part_name.trim_start_matches('/').to_ascii_lowercase();
                if self
                    .entry_names
                    .iter()
                    .any(|entry| entry.eq_ignore_ascii_case(&normalized))
                {
                    parts.insert(normalized);
                }
            }
        }
        Ok(parts)
    }

    pub fn unique_image_asset_part_count(
        &mut self,
        context: &ProbeContext,
        prefix: &str,
    ) -> Result<usize> {
        let content_types = self.part_content_types(context)?;
        Ok(count_unique_typed_parts(
            &self.entry_names,
            prefix,
            &content_types,
            AssetContentKind::Image,
        ))
    }

    pub fn unique_media_asset_part_count(
        &mut self,
        context: &ProbeContext,
        prefix: &str,
    ) -> Result<usize> {
        let content_types = self.part_content_types(context)?;
        Ok(count_unique_typed_parts(
            &self.entry_names,
            prefix,
            &content_types,
            AssetContentKind::AudioVideo,
        ))
    }

    fn part_content_types(&mut self, context: &ProbeContext) -> Result<PartContentTypes> {
        if let Some(content_types) = &self.content_types_cache {
            return Ok(content_types.clone());
        }
        let xml = self
            .read_text(context, "[Content_Types].xml")?
            .ok_or_else(|| {
                DeckProbeError::MalformedInput("missing [Content_Types].xml".to_owned())
            })?;
        let content_types = parse_part_content_types(&xml)?;
        self.content_types_cache = Some(content_types.clone());
        Ok(content_types)
    }
}

#[derive(Debug, Clone, Copy)]
enum AssetContentKind {
    Image,
    AudioVideo,
}

fn count_unique_typed_parts(
    names: &[String],
    prefix: &str,
    content_types: &PartContentTypes,
    kind: AssetContentKind,
) -> usize {
    let prefix = prefix.to_ascii_lowercase();
    names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .filter(|name| name.starts_with(&prefix))
        .filter(|name| {
            let content_type = content_types.overrides.get(name).or_else(|| {
                name.rsplit_once('.')
                    .and_then(|(_, extension)| content_types.defaults.get(extension))
            });
            content_type.is_some_and(|content_type| match kind {
                AssetContentKind::Image => content_type.starts_with("image/"),
                AssetContentKind::AudioVideo => {
                    content_type.starts_with("audio/") || content_type.starts_with("video/")
                }
            })
        })
        .collect::<BTreeSet<_>>()
        .len()
}

#[derive(Debug, Default)]
struct RelationshipFacts {
    has_external: bool,
    has_embedded: bool,
    has_signature: bool,
}

fn parse_relationship_facts(xml: &str, source: &str) -> Result<RelationshipFacts> {
    let mut reader = Reader::from_str(xml);
    let mut facts = RelationshipFacts::default();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if local_name(event.name().as_ref()) == "relationship" =>
            {
                let mut relationship_type = String::new();
                let mut target_mode = String::new();
                for attribute in event.attributes() {
                    let attribute = attribute.map_err(|error| {
                        DeckProbeError::MalformedInput(format!(
                            "invalid relationship attributes in {source}: {error}"
                        ))
                    })?;
                    let key = local_name(attribute.key.as_ref());
                    let value = attribute
                        .decode_and_unescape_value(reader.decoder())
                        .map_err(|error| {
                            DeckProbeError::MalformedInput(format!(
                                "invalid relationship attribute in {source}: {error}"
                            ))
                        })?
                        .into_owned();
                    match key.as_str() {
                        "type" => relationship_type = value,
                        "targetmode" => target_mode = value,
                        _ => {}
                    }
                }

                let relationship_type = relationship_type.to_ascii_lowercase();
                let external = target_mode.eq_ignore_ascii_case("external");
                facts.has_external |= external;
                facts.has_embedded |= !external
                    && (relationship_type.ends_with("/oleobject")
                        || relationship_type.ends_with("/package"));
                facts.has_signature |=
                    relationship_type.contains("/relationships/digital-signature/");
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(DeckProbeError::MalformedInput(format!(
                    "invalid relationships XML in {source}: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(facts)
}

fn digital_signature_overrides(xml: &str) -> Result<Vec<String>> {
    const SIGNATURE_CONTENT_TYPE: &str =
        "application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml";
    Ok(parse_part_content_types(xml)?
        .overrides
        .into_iter()
        .filter_map(|(part_name, content_type)| {
            (content_type == SIGNATURE_CONTENT_TYPE).then_some(part_name)
        })
        .collect())
}

fn parse_part_content_types(xml: &str) -> Result<PartContentTypes> {
    let mut reader = Reader::from_str(xml);
    let mut content_types = PartContentTypes::default();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                let element_name = local_name(event.name().as_ref());
                if !matches!(element_name.as_str(), "default" | "override") {
                    continue;
                }
                let mut part_name = None;
                let mut extension = None;
                let mut content_type = None;
                for attribute in event.attributes() {
                    let attribute = attribute.map_err(|error| {
                        DeckProbeError::MalformedInput(format!(
                            "invalid [Content_Types].xml attributes: {error}"
                        ))
                    })?;
                    let key = local_name(attribute.key.as_ref());
                    let value = attribute
                        .decode_and_unescape_value(reader.decoder())
                        .map_err(|error| {
                            DeckProbeError::MalformedInput(format!(
                                "invalid [Content_Types].xml attribute: {error}"
                            ))
                        })?
                        .into_owned();
                    match key.as_str() {
                        "partname" => part_name = Some(value),
                        "extension" => extension = Some(value),
                        "contenttype" => content_type = Some(value),
                        _ => {}
                    }
                }

                let Some(content_type) = content_type.map(|value| value.to_ascii_lowercase())
                else {
                    continue;
                };
                if element_name == "default" {
                    if let Some(extension) = extension {
                        content_types
                            .defaults
                            .insert(extension.to_ascii_lowercase(), content_type);
                    }
                } else if let Some(part_name) = part_name {
                    content_types.overrides.insert(
                        part_name.trim_start_matches('/').to_ascii_lowercase(),
                        content_type,
                    );
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(DeckProbeError::MalformedInput(format!(
                    "invalid [Content_Types].xml: {error}"
                )));
            }
            _ => {}
        }
    }
    Ok(content_types)
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

pub fn office_target_specs() -> Vec<TargetSpec> {
    use TargetScope::Office;
    use deckprobe_core::ProbeLevel::{Header, Metadata};
    vec![
        TargetSpec::new(
            "office.document_kind",
            "Office family: word/excel/powerpoint",
            "string",
            Office,
            Header,
        ),
        TargetSpec::new(
            "office.package_entry_count",
            "OPC ZIP entry count",
            "u64",
            Office,
            Metadata,
        ),
        TargetSpec::new(
            "office.conformance",
            "OOXML strict/transitional profile",
            "string",
            Office,
            Metadata,
        ),
    ]
}

pub fn common_path_targets() -> &'static [&'static str] {
    &[
        "document.format",
        "document.format_profile",
        "document.mime_type",
        "document.file_size",
        "document.extension",
        "document.extension_matches",
        "security.encrypted",
        "office.document_kind",
    ]
}

pub fn core_properties_targets() -> &'static [&'static str] {
    &[
        "document.title",
        "document.subject",
        "document.author",
        "document.keywords",
        "document.description",
        "document.created_at",
        "document.modified_at",
    ]
}

pub fn app_properties_targets() -> &'static [&'static str] {
    &["document.application", "document.application_version"]
}

pub fn inventory_targets() -> &'static [&'static str] {
    &[
        "security.has_macros",
        "office.package_entry_count",
        "office.conformance",
    ]
}

pub fn readability_security_targets() -> &'static [&'static str] {
    &["security.password_protected"]
}

pub fn package_security_targets() -> &'static [&'static str] {
    &[
        "security.has_digital_signature",
        "security.has_external_relationships",
        "security.has_embedded_files",
    ]
}

pub fn deep_security_targets() -> &'static [&'static str] {
    &[
        "security.signature_count",
        "security.has_javascript",
        "security.active_content_risk",
    ]
}

pub fn run_common_path<R: Read + Seek>(
    path_id: &str,
    session: &mut OoxmlSession<R>,
    context: &ProbeContext,
    profile: &FormatProfile,
) -> Result<Vec<Evidence>> {
    match path_id {
        "ooxml.identity" => Ok(identity_path_evidence(context, profile)),
        "ooxml.core_properties" => {
            let properties = session
                .read_text(context, "docProps/core.xml")?
                .map(|xml| element_text_map(&xml))
                .unwrap_or_default();
            let mappings = [
                ("document.title", "title"),
                ("document.subject", "subject"),
                ("document.author", "creator"),
                ("document.keywords", "keywords"),
                ("document.description", "description"),
                ("document.created_at", "created"),
                ("document.modified_at", "modified"),
            ];
            Ok(mappings
                .into_iter()
                .map(|(target, property)| {
                    property_evidence(
                        target,
                        properties.get(property),
                        path_id,
                        "docProps/core.xml",
                    )
                })
                .collect())
        }
        "ooxml.app_properties" => {
            let properties = session
                .read_text(context, "docProps/app.xml")?
                .map(|xml| element_text_map(&xml))
                .unwrap_or_default();
            Ok(vec![
                property_evidence(
                    "document.application",
                    properties.get("application"),
                    path_id,
                    "docProps/app.xml",
                ),
                property_evidence(
                    "document.application_version",
                    properties.get("appversion"),
                    path_id,
                    "docProps/app.xml",
                ),
            ])
        }
        "ooxml.package_inventory" => {
            let macros = session.has_suffix("vbaproject.bin");
            let conformance = session
                .read_text(context, "[Content_Types].xml")?
                .map(|xml| {
                    if xml.to_ascii_lowercase().contains("purl.oclc.org/ooxml") {
                        "strict"
                    } else {
                        "transitional"
                    }
                })
                .unwrap_or("unknown");
            Ok(vec![
                Evidence::resolved(
                    "security.has_macros",
                    macros,
                    Confidence::Exact,
                    path_id,
                    "OPC entry inventory",
                ),
                Evidence::resolved(
                    "office.package_entry_count",
                    json!(session.entry_count()),
                    Confidence::Exact,
                    path_id,
                    "ZIP central directory",
                ),
                Evidence::resolved(
                    "office.conformance",
                    conformance,
                    Confidence::High,
                    path_id,
                    "[Content_Types].xml namespaces",
                ),
            ])
        }
        "ooxml.readability_security" => Ok(vec![Evidence::resolved(
            "security.password_protected",
            false,
            Confidence::Exact,
            path_id,
            "readable unencrypted OPC package",
        )]),
        "ooxml.package_security" => {
            let inventory = session.security_inventory(context)?;
            Ok(vec![
                Evidence::resolved(
                    "security.has_digital_signature",
                    inventory.has_signature_structure,
                    Confidence::Exact,
                    path_id,
                    "OPC digital-signature parts and relationships",
                ),
                Evidence::resolved(
                    "security.has_external_relationships",
                    inventory.has_external_relationships,
                    Confidence::Exact,
                    path_id,
                    "all OPC .rels parts scanned for TargetMode=External",
                ),
                Evidence::resolved(
                    "security.has_embedded_files",
                    inventory.has_embedded_files,
                    Confidence::Exact,
                    path_id,
                    "OPC embeddings and internal oleObject/package relationships",
                ),
            ])
        }
        "ooxml.deep_security" => {
            let inventory = session.security_inventory(context)?;
            Ok(vec![
                Evidence::resolved(
                    "security.signature_count",
                    json!(inventory.signature_count),
                    Confidence::Exact,
                    path_id,
                    "unique OPC digital-signature XML parts",
                ),
                Evidence::unresolved(
                    "security.has_javascript",
                    deckprobe_core::TargetStatus::Unsupported,
                    path_id,
                ),
                Evidence::resolved(
                    "security.active_content_risk",
                    active_content_risk(&inventory),
                    Confidence::Exact,
                    path_id,
                    "rule: macros=high; embedded files=medium; external relationships=low; otherwise none",
                ),
            ])
        }
        _ => Err(DeckProbeError::InvalidRequest(format!(
            "unknown shared OOXML path: {path_id}"
        ))),
    }
}

fn active_content_risk(inventory: &OoxmlSecurityInventory) -> &'static str {
    if inventory.has_macros {
        "high"
    } else if inventory.has_embedded_files {
        "medium"
    } else if inventory.has_external_relationships {
        "low"
    } else {
        "none"
    }
}

pub fn identity_path_evidence(context: &ProbeContext, profile: &FormatProfile) -> Vec<Evidence> {
    let path_id = "ooxml.identity";
    let mut evidence = deckprobe_core::identity_evidence(context, profile, path_id);
    evidence.push(Evidence::resolved(
        "security.encrypted",
        false,
        Confidence::Exact,
        path_id,
        "readable OPC ZIP",
    ));
    evidence.push(Evidence::resolved(
        "office.document_kind",
        profile.driver,
        Confidence::Exact,
        path_id,
        "main part family",
    ));
    evidence
}

pub fn element_text_map(xml: &str) -> BTreeMap<String, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<String> = Vec::new();
    let mut values = BTreeMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => stack.push(local_name(event.name().as_ref())),
            Ok(Event::Text(event)) => {
                if let Some(name) = stack.last() {
                    let decoded = event.decode().unwrap_or(Cow::Borrowed(""));
                    let value = match quick_xml::escape::unescape(&decoded) {
                        Ok(value) => value.into_owned(),
                        Err(_) => decoded.into_owned(),
                    };
                    if !value.trim().is_empty() {
                        values.insert(name.clone(), value.trim().to_owned());
                    }
                }
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    values
}

pub fn count_start_elements(xml: &str, wanted: &str) -> u64 {
    let mut reader = Reader::from_str(xml);
    let mut count = 0;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                if local_name(event.name().as_ref()) == wanted {
                    count += 1;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    count
}

pub fn start_elements_with_attributes(xml: &str, wanted: &str) -> Vec<BTreeMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut items = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if local_name(event.name().as_ref()) == wanted =>
            {
                let mut attributes = BTreeMap::new();
                for attribute in event.attributes().flatten() {
                    let key = local_name(attribute.key.as_ref());
                    if let Ok(value) = attribute.decode_and_unescape_value(reader.decoder()) {
                        attributes.insert(key, value.into_owned());
                    }
                }
                items.push(attributes);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    items
}

pub fn first_element_attributes(xml: &str, wanted: &str) -> Option<BTreeMap<String, String>> {
    start_elements_with_attributes(xml, wanted)
        .into_iter()
        .next()
}

pub fn count_unique_parts(names: &[String], prefix: &str, suffix: &str) -> usize {
    let prefix = prefix.to_ascii_lowercase();
    let suffix = suffix.to_ascii_lowercase();
    names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .filter(|name| name.starts_with(&prefix) && name.ends_with(&suffix))
        .collect::<BTreeSet<_>>()
        .len()
}

fn property_evidence(target: &str, value: Option<&String>, path: &str, source: &str) -> Evidence {
    match value {
        Some(value) => Evidence::resolved(target, value.clone(), Confidence::High, path, source),
        None => Evidence::unresolved(target, deckprobe_core::TargetStatus::Unknown, path),
    }
}

fn local_name(bytes: &[u8]) -> String {
    let start = bytes
        .iter()
        .rposition(|value| *value == b':')
        .map_or(0, |index| index + 1);
    String::from_utf8_lossy(&bytes[start..]).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationship_scan_distinguishes_external_and_embedded_targets() {
        let xml = r#"
            <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
              <Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.test" TargetMode="External"/>
              <Relationship Id="r2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/package" Target="../embeddings/item.xlsx"/>
              <Relationship Id="r3" Type="http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature" Target="sig1.xml"/>
            </Relationships>
        "#;
        let facts = parse_relationship_facts(xml, "test.rels").unwrap();
        assert!(facts.has_external);
        assert!(facts.has_embedded);
        assert!(facts.has_signature);
    }

    #[test]
    fn signature_overrides_only_return_signature_content_types() {
        let xml = r#"
            <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
              <Override PartName="/_xmlsignatures/sig1.xml" ContentType="application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml"/>
              <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
            </Types>
        "#;
        assert_eq!(
            digital_signature_overrides(xml).unwrap(),
            vec!["_xmlsignatures/sig1.xml"]
        );
    }

    #[test]
    fn asset_inventory_counts_unique_parts_by_semantic_type() {
        let names = vec![
            "ppt/media/image1.PNG".to_owned(),
            "PPT/MEDIA/IMAGE1.png".to_owned(),
            "ppt/media/video1.mp4".to_owned(),
            "ppt/media/audio1.wav".to_owned(),
            "ppt/media/data.bin".to_owned(),
            "ppt/charts/chart1.xml".to_owned(),
            "ppt/charts/chart1.xml".to_owned(),
        ];
        let content_types = parse_part_content_types(
            r#"
                <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
                  <Default Extension="png" ContentType="image/png"/>
                  <Default Extension="mp4" ContentType="video/mp4"/>
                  <Default Extension="wav" ContentType="audio/wav"/>
                  <Override PartName="/ppt/media/data.bin" ContentType="image/svg+xml"/>
                </Types>
            "#,
        )
        .unwrap();
        assert_eq!(
            count_unique_typed_parts(
                &names,
                "ppt/media/",
                &content_types,
                AssetContentKind::Image
            ),
            2
        );
        assert_eq!(
            count_unique_typed_parts(
                &names,
                "ppt/media/",
                &content_types,
                AssetContentKind::AudioVideo
            ),
            2
        );
        assert_eq!(count_unique_parts(&names, "ppt/charts/chart", ".xml"), 1);
    }

    #[test]
    fn active_content_rule_uses_highest_risk_signal() {
        let inventory = |macros, embedded, external| OoxmlSecurityInventory {
            has_macros: macros,
            signature_count: 0,
            has_signature_structure: false,
            has_external_relationships: external,
            has_embedded_files: embedded,
        };
        assert_eq!(active_content_risk(&inventory(false, false, false)), "none");
        assert_eq!(active_content_risk(&inventory(false, false, true)), "low");
        assert_eq!(active_content_risk(&inventory(false, true, true)), "medium");
        assert_eq!(active_content_risk(&inventory(true, true, true)), "high");
    }
}
