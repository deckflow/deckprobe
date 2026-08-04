use std::collections::{BTreeMap, BTreeSet};

use deckprobe_core::{
    Confidence, DeckProbeError, Evidence, ExecutionPlan, FormatDriver, FormatProfile, OptionSpec,
    PathDescriptor, ProbeContext, ProbeLevel, ProbeRequest, Result, TargetScope, TargetSpec,
    common_target_specs,
};
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde_json::json;

pub const PDF_PROFILE: FormatProfile = FormatProfile {
    driver: "pdf",
    format: "pdf",
    profile: "pdf",
    mime_type: "application/pdf",
    extensions: &["pdf"],
};

pub struct PdfDriver;

impl PdfDriver {
    pub fn new() -> Self {
        Self
    }

    fn pdf_targets() -> Vec<TargetSpec> {
        use ProbeLevel::{Header, Metadata};
        use TargetScope::Format;
        vec![
            TargetSpec::new(
                "pdf.version",
                "PDF header version",
                "string",
                Format,
                Header,
            ),
            TargetSpec::new(
                "pdf.linearized",
                "Linearization dictionary detected",
                "bool",
                Format,
                Header,
            ),
            TargetSpec::new(
                "pdf.page_count",
                "Page tree leaf count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "pdf.object_count",
                "Loaded indirect object count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "pdf.xref_type",
                "table/stream/hybrid/unknown",
                "string",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "pdf.repaired",
                "Whether bounded safe xref recovery was used",
                "bool",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "pdf.annotation_count",
                "Annotation dictionary count across all pages",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "pdf.form_field_count",
                "Terminal interactive form field count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "pdf.attachment_count",
                "Embedded file-specification attachment count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "pdf.has_xmp",
                "Catalog XMP metadata stream is present",
                "bool",
                Format,
                Metadata,
            ),
        ]
    }

    fn feature_targets() -> &'static [&'static str] {
        &[
            "security.password_protected",
            "security.has_digital_signature",
            "security.has_external_relationships",
            "security.has_embedded_files",
            "pdf.annotation_count",
            "pdf.form_field_count",
            "pdf.attachment_count",
            "pdf.has_xmp",
        ]
    }

    fn deep_feature_targets() -> &'static [&'static str] {
        &[
            "security.password_protected",
            "security.has_digital_signature",
            "security.has_external_relationships",
            "security.has_embedded_files",
            "security.signature_count",
            "security.has_javascript",
            "pdf.annotation_count",
            "pdf.form_field_count",
            "pdf.attachment_count",
            "pdf.has_xmp",
        ]
    }

    fn all_feature_targets() -> &'static [&'static str] {
        &[
            "security.password_protected",
            "security.has_digital_signature",
            "security.has_external_relationships",
            "security.has_embedded_files",
            "security.signature_count",
            "security.has_javascript",
            "security.active_content_risk",
            "pdf.annotation_count",
            "pdf.form_field_count",
            "pdf.attachment_count",
            "pdf.has_xmp",
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
            "pdf.version",
            "pdf.linearized",
        ]
    }

    fn metadata_targets() -> &'static [&'static str] {
        &[
            "document.title",
            "document.subject",
            "document.author",
            "document.keywords",
            "document.description",
            "document.created_at",
            "document.modified_at",
            "document.application",
            "document.application_version",
        ]
    }
}

impl Default for PdfDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatDriver for PdfDriver {
    fn id(&self) -> &'static str {
        "pdf"
    }

    fn profile(&self) -> &FormatProfile {
        &PDF_PROFILE
    }

    fn targets(&self) -> Vec<TargetSpec> {
        let mut targets = common_target_specs();
        targets.extend(Self::pdf_targets());
        targets
    }

    fn options(&self) -> Vec<OptionSpec> {
        vec![
            OptionSpec {
                key: "pdf.repair_xref".to_owned(),
                description: "Allow the parser's bounded safe xref recovery".to_owned(),
                value_type: "enum".to_owned(),
                default: "safe".to_owned(),
                allowed: vec!["none".to_owned(), "safe".to_owned()],
            },
            OptionSpec {
                key: "pdf.max_objects".to_owned(),
                description: "Reject a parsed PDF above this object count".to_owned(),
                value_type: "u64".to_owned(),
                default: "100000".to_owned(),
                allowed: vec![],
            },
        ]
    }

    fn default_targets(&self, level: ProbeLevel) -> BTreeSet<String> {
        let mut targets = Self::identity_targets()
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<BTreeSet<_>>();
        if level >= ProbeLevel::Metadata {
            targets.extend(
                [
                    "document.title",
                    "document.author",
                    "document.application",
                    "security.encrypted",
                    "pdf.page_count",
                    "pdf.object_count",
                    "pdf.xref_type",
                    "pdf.repaired",
                ]
                .into_iter()
                .map(str::to_owned),
            );
        }
        targets
    }

    fn paths(&self, request: &ProbeRequest) -> Result<Vec<PathDescriptor>> {
        self.validate_options(request)?;
        Ok(vec![
            PathDescriptor::new(
                "pdf.header",
                Self::identity_targets(),
                ProbeLevel::Header,
                Confidence::Exact,
                1,
            ),
            PathDescriptor::new(
                "pdf.document_structure",
                &[
                    "security.encrypted",
                    "security.has_macros",
                    "pdf.page_count",
                    "pdf.object_count",
                    "pdf.xref_type",
                    "pdf.repaired",
                ],
                ProbeLevel::Metadata,
                Confidence::Exact,
                100,
            ),
            PathDescriptor::new(
                "pdf.info_dictionary",
                Self::metadata_targets(),
                ProbeLevel::Metadata,
                Confidence::High,
                100,
            ),
            PathDescriptor::new(
                "pdf.document_features",
                Self::feature_targets(),
                ProbeLevel::Metadata,
                Confidence::Exact,
                100,
            ),
            PathDescriptor::new(
                "pdf.deep_document_features",
                Self::deep_feature_targets(),
                ProbeLevel::Deep,
                Confidence::Exact,
                105,
            ),
            PathDescriptor::new(
                "pdf.active_content_assessment",
                Self::all_feature_targets(),
                ProbeLevel::Deep,
                Confidence::High,
                110,
            ),
        ])
    }

    fn validate_options(&self, request: &ProbeRequest) -> Result<()> {
        for (key, value) in &request.format_options {
            match key.as_str() {
                "pdf.repair_xref" if ["none", "safe"].contains(&value.as_str()) => {}
                "pdf.max_objects" if value.parse::<u64>().is_ok() => {}
                "pdf.repair_xref" | "pdf.max_objects" => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "invalid {key}: {value}"
                    )));
                }
                _ => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "unknown PDF option: {key}"
                    )));
                }
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
        let needs_document = plan.paths.iter().any(|path| path != "pdf.header");
        let session = needs_document
            .then(|| PdfSession::open(context, request))
            .transpose()?;
        let mut output = Vec::new();
        for path in &plan.paths {
            context.check_time()?;
            match path.as_str() {
                "pdf.header" => {
                    let owned;
                    let bytes = if let Some(session) = &session {
                        session.bytes.as_slice()
                    } else {
                        owned = context.read_prefix(8192)?;
                        owned.as_slice()
                    };
                    output.extend(deckprobe_core::identity_evidence(
                        context,
                        &PDF_PROFILE,
                        path,
                    ));
                    let version = pdf_version(bytes).ok_or_else(|| {
                        DeckProbeError::MalformedInput("missing PDF header".to_owned())
                    })?;
                    output.push(Evidence::resolved(
                        "pdf.version",
                        version,
                        Confidence::Exact,
                        path,
                        "%PDF header",
                    ));
                    output.push(Evidence::resolved(
                        "pdf.linearized",
                        contains_ascii(bytes, b"/Linearized"),
                        Confidence::High,
                        path,
                        "first 8 KiB",
                    ));
                }
                "pdf.document_structure" => {
                    let session = session.as_ref().expect("PDF session");
                    let encrypted = session.document.trailer.get(b"Encrypt").is_ok();
                    output.extend([
                        Evidence::resolved(
                            "security.encrypted",
                            encrypted,
                            Confidence::Exact,
                            path,
                            "PDF trailer",
                        ),
                        Evidence::resolved(
                            "security.has_macros",
                            false,
                            Confidence::Exact,
                            path,
                            "PDF does not contain Office VBA projects",
                        ),
                        Evidence::resolved(
                            "pdf.page_count",
                            json!(session.document.get_pages().len()),
                            Confidence::Exact,
                            path,
                            "PDF page tree",
                        ),
                        Evidence::resolved(
                            "pdf.object_count",
                            json!(session.document.objects.len()),
                            Confidence::Exact,
                            path,
                            "PDF xref/object map",
                        ),
                        Evidence::resolved(
                            "pdf.xref_type",
                            detect_xref_type(&session.bytes),
                            Confidence::Exact,
                            path,
                            "xref syntax",
                        ),
                        Evidence::resolved(
                            "pdf.repaired",
                            session.repaired,
                            Confidence::Exact,
                            path,
                            session.repair_source,
                        ),
                    ]);
                }
                "pdf.info_dictionary" => {
                    let session = session.as_ref().expect("PDF session");
                    let info = info_dictionary(&session.document);
                    for (target, key) in [
                        ("document.title", b"Title".as_slice()),
                        ("document.subject", b"Subject".as_slice()),
                        ("document.author", b"Author".as_slice()),
                        ("document.keywords", b"Keywords".as_slice()),
                        ("document.description", b"Description".as_slice()),
                        ("document.created_at", b"CreationDate".as_slice()),
                        ("document.modified_at", b"ModDate".as_slice()),
                        ("document.application", b"Producer".as_slice()),
                        ("document.application_version", b"Creator".as_slice()),
                    ] {
                        output.push(
                            match info.as_ref().and_then(|dictionary| {
                                object_text(&session.document, dictionary.get(key).ok()?)
                            }) {
                                Some(value) => Evidence::resolved(
                                    target,
                                    value,
                                    Confidence::High,
                                    path,
                                    "PDF Info dictionary",
                                ),
                                None => Evidence::unresolved(
                                    target,
                                    deckprobe_core::TargetStatus::Unknown,
                                    path,
                                ),
                            },
                        );
                    }
                }
                "pdf.document_features"
                | "pdf.deep_document_features"
                | "pdf.active_content_assessment" => {
                    let session = session.as_ref().expect("PDF session");
                    output.extend(feature_evidence(session, request, path));
                }
                other => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "unknown PDF path: {other}"
                    )));
                }
            }
        }
        Ok(output)
    }
}

struct PdfSession {
    bytes: Vec<u8>,
    document: Document,
    repaired: bool,
    repair_source: &'static str,
}

impl PdfSession {
    fn open(context: &mut ProbeContext, request: &ProbeRequest) -> Result<Self> {
        let max_objects = request
            .format_options
            .get("pdf.max_objects")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(100_000);
        let bytes = context.read_all()?;
        let repair = request
            .format_options
            .get("pdf.repair_xref")
            .map(String::as_str)
            .unwrap_or("safe");
        let (bytes, document, repaired, repair_source) = match Document::load_mem(&bytes) {
            Ok(document) => (bytes, document, false, "none"),
            Err(original_error) if repair == "safe" => {
                context.check_time()?;
                let normalized = normalize_xref_lines(&bytes);
                if normalized != bytes {
                    if let Ok(document) = Document::load_mem(&normalized) {
                        (normalized, document, true, "normalized xref table")
                    } else {
                        rebuild_and_load(context, &bytes, max_objects, &original_error.to_string())?
                    }
                } else {
                    rebuild_and_load(context, &bytes, max_objects, &original_error.to_string())?
                }
            }
            Err(error) => {
                return Err(DeckProbeError::MalformedInput(format!(
                    "cannot parse PDF: {error}"
                )));
            }
        };
        if document.objects.len() > max_objects {
            return Err(DeckProbeError::BudgetExceeded(format!(
                "PDF object count {} exceeds pdf.max_objects {max_objects}",
                document.objects.len()
            )));
        }
        Ok(Self {
            bytes,
            document,
            repaired,
            repair_source,
        })
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PdfFeatureScan {
    signature_count: u64,
    has_javascript: bool,
    has_external_relationships: bool,
    has_embedded_files: bool,
    attachment_count: u64,
    has_launch_action: bool,
    has_data_action: bool,
    has_active_media: bool,
}

fn feature_evidence(session: &PdfSession, request: &ProbeRequest, path: &str) -> Vec<Evidence> {
    let mut output = Vec::new();
    if request.targets.contains("security.password_protected") {
        output.push(password_protection_evidence(&session.document, path));
    }

    let content_available = document_content_available(&session.document);
    let scan = content_available.then(|| scan_document_features(&session.document));

    push_optional_evidence(
        &mut output,
        request,
        "security.has_digital_signature",
        scan.as_ref()
            .map(|features| json!(features.signature_count > 0)),
        Confidence::Exact,
        path,
        "PDF signature dictionaries; cryptographic validity was not checked",
    );
    push_optional_evidence(
        &mut output,
        request,
        "security.has_external_relationships",
        scan.as_ref()
            .map(|features| json!(features.has_external_relationships)),
        Confidence::Exact,
        path,
        "PDF external action/file-spec dictionaries; no external resource was fetched",
    );
    push_optional_evidence(
        &mut output,
        request,
        "security.has_embedded_files",
        scan.as_ref()
            .map(|features| json!(features.has_embedded_files)),
        Confidence::Exact,
        path,
        "PDF EmbeddedFiles name tree, file specifications, and embedded-file streams",
    );
    push_optional_evidence(
        &mut output,
        request,
        "security.signature_count",
        scan.as_ref()
            .map(|features| json!(features.signature_count)),
        Confidence::Exact,
        path,
        "PDF signature dictionaries; cryptographic validity was not checked",
    );
    push_optional_evidence(
        &mut output,
        request,
        "security.has_javascript",
        scan.as_ref().map(|features| json!(features.has_javascript)),
        Confidence::Exact,
        path,
        "PDF JavaScript action or JavaScript name tree",
    );

    if request.targets.contains("security.active_content_risk") {
        output.push(match scan.as_ref() {
            Some(features) => Evidence::resolved(
                "security.active_content_risk",
                active_content_risk(features),
                Confidence::High,
                path,
                "rule: high=JavaScript/Launch; medium=data action/active media; low=external relationship/embedded file",
            ),
            None => Evidence::unresolved(
                "security.active_content_risk",
                deckprobe_core::TargetStatus::Unknown,
                path,
            ),
        });
    }

    let annotation_count = content_available
        .then(|| page_annotation_count(&session.document))
        .flatten();
    push_optional_evidence(
        &mut output,
        request,
        "pdf.annotation_count",
        annotation_count.map(|value| json!(value)),
        Confidence::Exact,
        path,
        "page-tree /Annots arrays",
    );

    let form_field_count = content_available
        .then(|| form_field_count(&session.document))
        .flatten();
    push_optional_evidence(
        &mut output,
        request,
        "pdf.form_field_count",
        form_field_count.map(|value| json!(value)),
        Confidence::Exact,
        path,
        "catalog /AcroForm field hierarchy",
    );
    push_optional_evidence(
        &mut output,
        request,
        "pdf.attachment_count",
        scan.as_ref()
            .map(|features| json!(features.attachment_count)),
        Confidence::Exact,
        path,
        "file-specification dictionaries with /EF entries",
    );

    let has_xmp = content_available
        .then(|| catalog_has_xmp(&session.document))
        .flatten();
    push_optional_evidence(
        &mut output,
        request,
        "pdf.has_xmp",
        has_xmp.map(|value| json!(value)),
        Confidence::Exact,
        path,
        "catalog /Metadata XML stream",
    );
    output
}

fn push_optional_evidence(
    output: &mut Vec<Evidence>,
    request: &ProbeRequest,
    target: &str,
    value: Option<serde_json::Value>,
    confidence: Confidence,
    path: &str,
    source: &str,
) {
    if !request.targets.contains(target) {
        return;
    }
    output.push(match value {
        Some(value) => Evidence::resolved(target, value, confidence, path, source),
        None => Evidence::unresolved(target, deckprobe_core::TargetStatus::Unknown, path),
    });
}

fn password_protection_evidence(document: &Document, path: &str) -> Evidence {
    if !document.is_encrypted() {
        return Evidence::resolved(
            "security.password_protected",
            false,
            Confidence::Exact,
            path,
            "PDF trailer has no /Encrypt entry",
        );
    }
    match document.authenticate_password("") {
        Ok(()) => Evidence::resolved(
            "security.password_protected",
            false,
            Confidence::Exact,
            path,
            "PDF encryption accepts the empty password",
        ),
        Err(lopdf::Error::Decryption(lopdf::encryption::DecryptionError::IncorrectPassword)) => {
            Evidence::resolved(
                "security.password_protected",
                true,
                Confidence::Exact,
                path,
                "PDF encryption rejects the empty password",
            )
        }
        Err(_) => Evidence::unresolved(
            "security.password_protected",
            deckprobe_core::TargetStatus::Unknown,
            path,
        ),
    }
}

fn document_content_available(document: &Document) -> bool {
    !document.is_encrypted() || document.encryption_state.is_some()
}

fn scan_document_features(document: &Document) -> PdfFeatureScan {
    let mut features = PdfFeatureScan::default();
    let mut pending = document.objects.values().collect::<Vec<_>>();
    while let Some(object) = pending.pop() {
        match object {
            Object::Array(values) => pending.extend(values),
            Object::Dictionary(dictionary) => {
                scan_feature_dictionary(dictionary, &mut features);
                pending.extend(dictionary.iter().map(|(_, value)| value));
            }
            Object::Stream(stream) => {
                scan_feature_dictionary(&stream.dict, &mut features);
                pending.extend(stream.dict.iter().map(|(_, value)| value));
            }
            Object::Null
            | Object::Boolean(_)
            | Object::Integer(_)
            | Object::Real(_)
            | Object::Name(_)
            | Object::String(_, _)
            | Object::Reference(_) => {}
        }
    }
    features
}

fn scan_feature_dictionary(dictionary: &Dictionary, features: &mut PdfFeatureScan) {
    let object_type = dictionary_name(dictionary, b"Type");
    let subtype = dictionary_name(dictionary, b"Subtype");
    let action = dictionary_name(dictionary, b"S");

    if matches!(object_type, Some(b"Sig" | b"DocTimeStamp"))
        || (dictionary.has(b"ByteRange") && dictionary.has(b"Contents"))
    {
        features.signature_count += 1;
    }
    if matches!(action, Some(b"JavaScript")) || dictionary.has(b"JavaScript") {
        features.has_javascript = true;
    }
    match action {
        Some(b"Launch") => {
            features.has_external_relationships = true;
            features.has_launch_action = true;
        }
        Some(b"SubmitForm" | b"ImportData") => {
            features.has_external_relationships = true;
            features.has_data_action = true;
        }
        Some(b"URI" | b"GoToR") => features.has_external_relationships = true,
        Some(b"Rendition" | b"Movie" | b"Sound") => features.has_active_media = true,
        _ => {}
    }

    let is_file_spec = matches!(object_type, Some(b"Filespec"));
    if dictionary.has(b"EF") {
        features.has_embedded_files = true;
        features.attachment_count += 1;
    } else if is_file_spec && (dictionary.has(b"F") || dictionary.has(b"UF")) {
        features.has_external_relationships = true;
    }
    if matches!(object_type, Some(b"EmbeddedFile")) || dictionary.has(b"EmbeddedFiles") {
        features.has_embedded_files = true;
    }

    // Rich-media annotations can execute media without using a named action dictionary.
    if matches!(
        subtype,
        Some(b"RichMedia" | b"Movie" | b"Sound" | b"Screen")
    ) {
        features.has_active_media = true;
    }
}

fn dictionary_name<'a>(dictionary: &'a Dictionary, key: &[u8]) -> Option<&'a [u8]> {
    dictionary.get(key).ok()?.as_name().ok()
}

fn active_content_risk(features: &PdfFeatureScan) -> &'static str {
    if features.has_javascript || features.has_launch_action {
        "high"
    } else if features.has_data_action || features.has_active_media {
        "medium"
    } else if features.has_external_relationships || features.has_embedded_files {
        "low"
    } else {
        "none"
    }
}

fn page_annotation_count(document: &Document) -> Option<u64> {
    let mut count = 0_u64;
    for page_id in document.get_pages().into_values() {
        let page = document.get_dictionary(page_id).ok()?;
        let Ok(annotations) = page.get(b"Annots") else {
            continue;
        };
        let (_, annotations) = document.dereference(annotations).ok()?;
        let annotations = annotations.as_array().ok()?;
        for annotation in annotations {
            let (_, annotation) = document.dereference(annotation).ok()?;
            annotation.as_dict().ok()?;
            count = count.checked_add(1)?;
        }
    }
    Some(count)
}

fn form_field_count(document: &Document) -> Option<u64> {
    let catalog = catalog_dictionary(document)?;
    let Ok(acro_form) = catalog.get(b"AcroForm") else {
        return Some(0);
    };
    let (_, acro_form) = document.dereference(acro_form).ok()?;
    let acro_form = acro_form.as_dict().ok()?;
    let fields = acro_form
        .get_deref(b"Fields", document)
        .ok()?
        .as_array()
        .ok()?;
    let mut active = BTreeSet::new();
    fields.iter().try_fold(0_u64, |total, field| {
        total.checked_add(count_terminal_form_field(document, field, &mut active, 0)?)
    })
}

fn count_terminal_form_field(
    document: &Document,
    field: &Object,
    active: &mut BTreeSet<ObjectId>,
    depth: usize,
) -> Option<u64> {
    if depth > 256 {
        return None;
    }
    let (object_id, field) = document.dereference(field).ok()?;
    if let Some(object_id) = object_id
        && !active.insert(object_id)
    {
        return None;
    }
    let result = (|| {
        let field = field.as_dict().ok()?;
        let Ok(kids) = field.get(b"Kids") else {
            return Some(1);
        };
        let (_, kids) = document.dereference(kids).ok()?;
        let kids = kids.as_array().ok()?;
        let mut field_children = Vec::new();
        for kid in kids {
            let (_, resolved) = document.dereference(kid).ok()?;
            let dictionary = resolved.as_dict().ok()?;
            let widget_only = matches!(dictionary_name(dictionary, b"Subtype"), Some(b"Widget"))
                && !dictionary.has(b"T")
                && !dictionary.has(b"FT")
                && !dictionary.has(b"Kids");
            if !widget_only {
                field_children.push(kid);
            }
        }
        if field_children.is_empty() {
            return Some(1);
        }
        field_children.iter().try_fold(0_u64, |total, child| {
            total.checked_add(count_terminal_form_field(
                document,
                child,
                active,
                depth + 1,
            )?)
        })
    })();
    if let Some(object_id) = object_id {
        active.remove(&object_id);
    }
    result
}

fn catalog_has_xmp(document: &Document) -> Option<bool> {
    let catalog = catalog_dictionary(document)?;
    let Ok(metadata) = catalog.get(b"Metadata") else {
        return Some(false);
    };
    let (_, metadata) = document.dereference(metadata).ok()?;
    let stream = metadata.as_stream().ok()?;
    (matches!(dictionary_name(&stream.dict, b"Type"), Some(b"Metadata"))
        && matches!(dictionary_name(&stream.dict, b"Subtype"), Some(b"XML")))
    .then_some(true)
}

fn catalog_dictionary(document: &Document) -> Option<&Dictionary> {
    let root = document.trailer.get(b"Root").ok()?;
    let (_, root) = document.dereference(root).ok()?;
    root.as_dict().ok()
}

fn normalize_xref_lines(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut start = 0;
    let mut in_xref = false;
    while start < bytes.len() {
        let end = bytes[start..]
            .iter()
            .position(|value| *value == b'\n')
            .map_or(bytes.len(), |offset| start + offset + 1);
        let mut content_end = end;
        while content_end > start && matches!(bytes[content_end - 1], b'\n' | b'\r') {
            content_end -= 1;
        }
        let line = &bytes[start..content_end];
        let trimmed = trim_ascii(line);
        if trimmed == b"xref" {
            in_xref = true;
        } else if trimmed == b"trailer" {
            in_xref = false;
        }
        output.extend_from_slice(line);
        if in_xref && is_short_xref_entry(trimmed) {
            output.push(b' ');
        }
        output.extend_from_slice(&bytes[content_end..end]);
        start = end;
    }
    output
}

fn is_short_xref_entry(line: &[u8]) -> bool {
    line.len() == 18
        && line[..10].iter().all(u8::is_ascii_digit)
        && line[10] == b' '
        && line[11..16].iter().all(u8::is_ascii_digit)
        && line[16] == b' '
        && matches!(line[17], b'n' | b'f')
}

fn rebuild_and_load(
    context: &ProbeContext,
    bytes: &[u8],
    max_objects: usize,
    original_error: &str,
) -> Result<(Vec<u8>, Document, bool, &'static str)> {
    if contains_ascii(bytes, b"/Encrypt") {
        return Err(DeckProbeError::MalformedInput(format!(
            "cannot safely reconstruct encrypted PDF xref: {original_error}"
        )));
    }
    let objects = scan_indirect_objects(context, bytes, max_objects)?;
    if objects.is_empty() {
        return Err(DeckProbeError::MalformedInput(format!(
            "cannot reconstruct PDF xref: {original_error}"
        )));
    }
    let root = find_reference_after(bytes, b"/Root")
        .or_else(|| {
            objects.iter().find_map(|(&(number, generation), &offset)| {
                let end = bytes[offset..]
                    .windows(6)
                    .position(|value| value == b"endobj")
                    .map_or(bytes.len(), |relative| offset + relative);
                contains_ascii(&bytes[offset..end], b"/Type /Catalog")
                    .then_some((number, generation))
            })
        })
        .ok_or_else(|| {
            DeckProbeError::MalformedInput(
                "cannot reconstruct PDF xref without a catalog root".to_owned(),
            )
        })?;

    let max_number = objects.keys().map(|(number, _)| *number).max().unwrap_or(0);
    if max_number as usize >= max_objects {
        return Err(DeckProbeError::BudgetExceeded(format!(
            "PDF object number {max_number} exceeds pdf.max_objects {max_objects}"
        )));
    }
    let mut repaired = bytes.to_vec();
    if !repaired.ends_with(b"\n") {
        repaired.push(b'\n');
    }
    let xref_offset = repaired.len();
    repaired.extend_from_slice(format!("xref\n0 {}\n", max_number + 1).as_bytes());
    repaired.extend_from_slice(b"0000000000 65535 f \n");
    for number in 1..=max_number {
        if let Some((&(_, generation), &offset)) = objects
            .iter()
            .find(|((object_number, _), _)| *object_number == number)
        {
            repaired.extend_from_slice(format!("{offset:010} {generation:05} n \n").as_bytes());
        } else {
            repaired.extend_from_slice(b"0000000000 65535 f \n");
        }
    }
    repaired.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root {} {} R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            max_number + 1,
            root.0,
            root.1
        )
        .as_bytes(),
    );
    context.check_time()?;
    let document = Document::load_mem(&repaired).map_err(|error| {
        DeckProbeError::MalformedInput(format!(
            "cannot parse PDF after bounded xref reconstruction: {error}; original: {original_error}"
        ))
    })?;
    Ok((repaired, document, true, "reconstructed xref table"))
}

fn scan_indirect_objects(
    context: &ProbeContext,
    bytes: &[u8],
    max_objects: usize,
) -> Result<BTreeMap<(u32, u16), usize>> {
    let mut objects = BTreeMap::new();
    let mut offset = 0;
    for line in bytes.split_inclusive(|value| *value == b'\n') {
        let tokens = trim_ascii(line)
            .split(|value| value.is_ascii_whitespace())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if tokens.len() >= 3
            && tokens[2] == b"obj"
            && let (Ok(number), Ok(generation)) = (
                std::str::from_utf8(tokens[0]).unwrap_or("").parse::<u32>(),
                std::str::from_utf8(tokens[1]).unwrap_or("").parse::<u16>(),
            )
        {
            objects.insert((number, generation), offset);
            if objects.len() > max_objects {
                return Err(DeckProbeError::BudgetExceeded(format!(
                    "PDF object scan exceeds pdf.max_objects {max_objects}"
                )));
            }
        }
        offset += line.len();
        if objects.len() % 1024 == 0 {
            context.check_time()?;
        }
    }
    Ok(objects)
}

fn find_reference_after(bytes: &[u8], marker: &[u8]) -> Option<(u32, u16)> {
    let start = bytes
        .windows(marker.len())
        .rposition(|window| window == marker)?
        + marker.len();
    let tokens = bytes[start..bytes.len().min(start + 64)]
        .split(|value| value.is_ascii_whitespace())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if tokens.len() < 3 || tokens[2] != b"R" {
        return None;
    }
    Some((
        std::str::from_utf8(tokens[0]).ok()?.parse().ok()?,
        std::str::from_utf8(tokens[1]).ok()?.parse().ok()?,
    ))
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn pdf_version(bytes: &[u8]) -> Option<String> {
    let start = bytes.windows(5).position(|value| value == b"%PDF-")? + 5;
    let tail = &bytes[start..bytes.len().min(start + 8)];
    let length = tail
        .iter()
        .position(|value| !value.is_ascii_digit() && *value != b'.')
        .unwrap_or(tail.len());
    Some(String::from_utf8_lossy(&tail[..length]).into_owned())
}

fn contains_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn detect_xref_type(bytes: &[u8]) -> &'static str {
    let table = contains_ascii(bytes, b"\nxref") || contains_ascii(bytes, b"\rxref");
    let stream = contains_ascii(bytes, b"/Type/XRef") || contains_ascii(bytes, b"/Type /XRef");
    match (table, stream) {
        (true, true) => "hybrid",
        (true, false) => "table",
        (false, true) => "stream",
        _ => "unknown",
    }
}

fn info_dictionary(document: &Document) -> Option<Dictionary> {
    let object = document.trailer.get(b"Info").ok()?;
    match object {
        Object::Reference(id) => document.get_dictionary(*id).ok().cloned(),
        Object::Dictionary(dictionary) => Some(dictionary.clone()),
        _ => None,
    }
}

fn object_text(document: &Document, object: &Object) -> Option<String> {
    let object = match object {
        Object::Reference(id) => document.get_object(*id).ok()?,
        value => value,
    };
    match object {
        Object::String(bytes, _) => Some(decode_pdf_string(bytes)),
        Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

fn decode_pdf_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&words)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &[u8]) -> Object {
        Object::Name(value.to_vec())
    }

    #[test]
    fn safe_normalization_accepts_short_but_standard_xref_lines() {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let catalog = pdf.len();
        pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        let pages = pdf.len();
        pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n");
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!(
                "xref\n0 3\n0000000000 65535 f\n{catalog:010} 00000 n\n{pages:010} 00000 n\ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
            )
            .as_bytes(),
        );
        assert!(Document::load_mem(&pdf).is_err());
        let normalized = normalize_xref_lines(&pdf);
        let document = Document::load_mem(&normalized).unwrap();
        assert_eq!(document.get_pages().len(), 0);
    }

    #[test]
    fn version_1_1_targets_do_not_change_pdf_defaults() {
        let defaults = PdfDriver.default_targets(ProbeLevel::Deep);
        for target in PdfDriver::all_feature_targets() {
            assert!(
                !defaults.contains(*target),
                "unexpected default target: {target}"
            );
        }
    }

    #[test]
    fn feature_scan_finds_signature_javascript_external_and_embedded_structures() {
        let mut document = Document::new();

        let mut signature = Dictionary::new();
        signature.set(b"Type".as_slice(), name(b"Sig"));
        signature.set(b"ByteRange".as_slice(), Object::Array(vec![]));
        signature.set(
            b"Contents".as_slice(),
            Object::String(vec![], Default::default()),
        );
        document
            .objects
            .insert((1, 0), Object::Dictionary(signature));

        let mut javascript = Dictionary::new();
        javascript.set(b"S".as_slice(), name(b"JavaScript"));
        javascript.set(
            b"JS".as_slice(),
            Object::String(b"app.alert('test')".to_vec(), Default::default()),
        );
        document
            .objects
            .insert((2, 0), Object::Dictionary(javascript));

        let mut external = Dictionary::new();
        external.set(b"S".as_slice(), name(b"URI"));
        external.set(
            b"URI".as_slice(),
            Object::String(b"https://example.invalid".to_vec(), Default::default()),
        );
        document
            .objects
            .insert((3, 0), Object::Dictionary(external));

        let mut embedded_reference = Dictionary::new();
        embedded_reference.set(b"F".as_slice(), Object::Reference((5, 0)));
        let mut file_spec = Dictionary::new();
        file_spec.set(b"Type".as_slice(), name(b"Filespec"));
        file_spec.set(b"EF".as_slice(), Object::Dictionary(embedded_reference));
        document
            .objects
            .insert((4, 0), Object::Dictionary(file_spec));

        let mut embedded_stream = Dictionary::new();
        embedded_stream.set(b"Type".as_slice(), name(b"EmbeddedFile"));
        document.objects.insert(
            (5, 0),
            Object::Stream(lopdf::Stream::new(embedded_stream, vec![1, 2, 3])),
        );

        let features = scan_document_features(&document);
        assert_eq!(features.signature_count, 1);
        assert!(features.has_javascript);
        assert!(features.has_external_relationships);
        assert!(features.has_embedded_files);
        assert_eq!(features.attachment_count, 1);
        assert_eq!(active_content_risk(&features), "high");
    }

    #[test]
    fn counts_annotations_terminal_fields_and_catalog_xmp() {
        let mut document = Document::new();

        let mut annotation = Dictionary::new();
        annotation.set(b"Type".as_slice(), name(b"Annot"));
        annotation.set(b"Subtype".as_slice(), name(b"Text"));
        document
            .objects
            .insert((4, 0), Object::Dictionary(annotation.clone()));
        document
            .objects
            .insert((5, 0), Object::Dictionary(annotation));

        let mut page = Dictionary::new();
        page.set(b"Type".as_slice(), name(b"Page"));
        page.set(b"Parent".as_slice(), Object::Reference((2, 0)));
        page.set(
            b"Annots".as_slice(),
            Object::Array(vec![Object::Reference((4, 0)), Object::Reference((5, 0))]),
        );
        document.objects.insert((3, 0), Object::Dictionary(page));

        let mut pages = Dictionary::new();
        pages.set(b"Type".as_slice(), name(b"Pages"));
        pages.set(b"Count".as_slice(), 1_i64);
        pages.set(
            b"Kids".as_slice(),
            Object::Array(vec![Object::Reference((3, 0))]),
        );
        document.objects.insert((2, 0), Object::Dictionary(pages));

        let mut first_field = Dictionary::new();
        first_field.set(b"FT".as_slice(), name(b"Tx"));
        first_field.set(
            b"T".as_slice(),
            Object::String(b"first".to_vec(), Default::default()),
        );
        let mut child_field = Dictionary::new();
        child_field.set(
            b"T".as_slice(),
            Object::String(b"second".to_vec(), Default::default()),
        );
        let mut widget = Dictionary::new();
        widget.set(b"Subtype".as_slice(), name(b"Widget"));
        let mut parent_field = Dictionary::new();
        parent_field.set(
            b"Kids".as_slice(),
            Object::Array(vec![
                Object::Dictionary(child_field),
                Object::Dictionary(widget),
            ]),
        );
        let mut acro_form = Dictionary::new();
        acro_form.set(
            b"Fields".as_slice(),
            Object::Array(vec![
                Object::Dictionary(first_field),
                Object::Dictionary(parent_field),
            ]),
        );

        let mut metadata = Dictionary::new();
        metadata.set(b"Type".as_slice(), name(b"Metadata"));
        metadata.set(b"Subtype".as_slice(), name(b"XML"));
        document.objects.insert(
            (6, 0),
            Object::Stream(lopdf::Stream::new(metadata, b"<x:xmpmeta/>".to_vec())),
        );

        let mut catalog = Dictionary::new();
        catalog.set(b"Type".as_slice(), name(b"Catalog"));
        catalog.set(b"Pages".as_slice(), Object::Reference((2, 0)));
        catalog.set(b"AcroForm".as_slice(), Object::Dictionary(acro_form));
        catalog.set(b"Metadata".as_slice(), Object::Reference((6, 0)));
        document.objects.insert((1, 0), Object::Dictionary(catalog));
        document
            .trailer
            .set(b"Root".as_slice(), Object::Reference((1, 0)));

        assert_eq!(page_annotation_count(&document), Some(2));
        assert_eq!(form_field_count(&document), Some(2));
        assert_eq!(catalog_has_xmp(&document), Some(true));
    }
}
