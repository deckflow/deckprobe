use std::collections::BTreeSet;

use deckprobe_core::{
    Confidence, DeckProbeError, Evidence, ExecutionPlan, FormatDriver, FormatProfile, OptionSpec,
    PathDescriptor, ProbeContext, ProbeLevel, ProbeRequest, Result, TargetScope, TargetSpec,
    common_target_specs,
};
use deckprobe_format_ooxml::{
    OoxmlSession, app_properties_targets, common_path_targets, core_properties_targets,
    count_unique_parts, deep_security_targets, element_text_map, first_element_attributes,
    identity_path_evidence, inventory_targets, office_target_specs, package_security_targets,
    readability_security_targets, run_common_path, start_elements_with_attributes,
};
use serde_json::json;

pub struct PowerPointDriver {
    profile: FormatProfile,
}

impl PowerPointDriver {
    pub fn new(profile: FormatProfile) -> Self {
        Self { profile }
    }

    fn powerpoint_targets() -> Vec<TargetSpec> {
        use ProbeLevel::Metadata;
        use TargetScope::Format;
        vec![
            TargetSpec::new(
                "powerpoint.slide_count",
                "Logical slide count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.hidden_slide_count",
                "Slides marked hidden",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.master_count",
                "Slide master part count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.layout_count",
                "Slide layout part count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.notes_slide_count",
                "Notes slide part count",
                "u64|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.slide_size",
                "Slide size in EMU and points",
                "object|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.aspect_ratio",
                "Reduced slide aspect ratio",
                "object|null",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.orientation",
                "landscape/portrait/square",
                "string|null",
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
            TargetSpec::new(
                "powerpoint.chart_part_count",
                "Unique PowerPoint chart XML part count (not chart instance count)",
                "u64",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.unique_image_asset_count",
                "Unique image asset part count in the PowerPoint package (not image instance count)",
                "u64",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.unique_media_asset_count",
                "Unique audio/video media asset part count (images excluded; not media instance count)",
                "u64",
                Format,
                Metadata,
            ),
            TargetSpec::new(
                "powerpoint.comment_part_count",
                "Unique PowerPoint comment XML part count (not logical comment count)",
                "u64",
                Format,
                Metadata,
            ),
        ]
    }

    fn slide_count_path(request: &ProbeRequest) -> &str {
        request
            .format_options
            .get("powerpoint.slide_count_path")
            .map(String::as_str)
            .unwrap_or("auto")
    }
}

impl FormatDriver for PowerPointDriver {
    fn id(&self) -> &'static str {
        "powerpoint"
    }

    fn profile(&self) -> &FormatProfile {
        &self.profile
    }

    fn targets(&self) -> Vec<TargetSpec> {
        let mut targets = common_target_specs();
        targets.extend(office_target_specs());
        targets.extend(Self::powerpoint_targets());
        targets
    }

    fn options(&self) -> Vec<OptionSpec> {
        vec![OptionSpec {
            key: "powerpoint.slide_count_path".to_owned(),
            description: "Use saved app property for speed or presentation.xml for exact count"
                .to_owned(),
            value_type: "enum".to_owned(),
            default: "auto".to_owned(),
            allowed: vec![
                "auto".to_owned(),
                "app-properties".to_owned(),
                "presentation-xml".to_owned(),
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
                    "powerpoint.slide_count",
                    "powerpoint.hidden_slide_count",
                    "powerpoint.master_count",
                    "powerpoint.layout_count",
                    "powerpoint.slide_size",
                    "powerpoint.aspect_ratio",
                    "powerpoint.orientation",
                    "powerpoint.presentation_kind",
                ]
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
                "powerpoint.profile",
                &["powerpoint.presentation_kind"],
                ProbeLevel::Metadata,
                Confidence::Exact,
                1,
            ),
            PathDescriptor::new(
                "powerpoint.part_inventory",
                &[
                    "powerpoint.master_count",
                    "powerpoint.layout_count",
                    "powerpoint.notes_slide_count",
                ],
                ProbeLevel::Metadata,
                Confidence::Exact,
                2,
            ),
            PathDescriptor::new(
                "powerpoint.asset_inventory",
                &[
                    "powerpoint.chart_part_count",
                    "powerpoint.unique_image_asset_count",
                    "powerpoint.unique_media_asset_count",
                    "powerpoint.comment_part_count",
                ],
                ProbeLevel::Metadata,
                Confidence::Exact,
                2,
            ),
        ];
        if matches!(Self::slide_count_path(request), "auto" | "app-properties") {
            paths.push(PathDescriptor::new(
                "powerpoint.app_statistics",
                &["powerpoint.slide_count"],
                ProbeLevel::Metadata,
                Confidence::High,
                4,
            ));
        }
        if matches!(Self::slide_count_path(request), "auto" | "presentation-xml") {
            paths.push(PathDescriptor::new(
                "powerpoint.presentation_xml",
                &[
                    "powerpoint.slide_count",
                    "powerpoint.hidden_slide_count",
                    "powerpoint.slide_size",
                    "powerpoint.aspect_ratio",
                    "powerpoint.orientation",
                ],
                ProbeLevel::Metadata,
                Confidence::Exact,
                12,
            ));
        }
        Ok(paths)
    }

    fn validate_options(&self, request: &ProbeRequest) -> Result<()> {
        for (key, value) in &request.format_options {
            if key != "powerpoint.slide_count_path" {
                return Err(DeckProbeError::InvalidRequest(format!(
                    "unknown PowerPoint option: {key}"
                )));
            }
            if !["auto", "app-properties", "presentation-xml"].contains(&value.as_str()) {
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
                "powerpoint.profile" => {
                    let kind = match self.profile.profile {
                        "ppsx" | "ppsm" => "show",
                        "potx" | "potm" => "template",
                        _ => "presentation",
                    };
                    output.push(Evidence::resolved(
                        "powerpoint.presentation_kind",
                        kind,
                        Confidence::Exact,
                        path,
                        "detected OOXML profile",
                    ));
                }
                "powerpoint.part_inventory" => {
                    let names = session.as_ref().expect("package session").entry_names();
                    let master_count = count_parts(names, "ppt/slideMasters/slideMaster", ".xml");
                    let layout_count = count_parts(names, "ppt/slideLayouts/slideLayout", ".xml");
                    let notes_count = count_parts(names, "ppt/notesSlides/notesSlide", ".xml");
                    output.extend([
                        Evidence::resolved(
                            "powerpoint.master_count",
                            json!(master_count),
                            Confidence::Exact,
                            path,
                            "OPC entry inventory",
                        ),
                        Evidence::resolved(
                            "powerpoint.layout_count",
                            json!(layout_count),
                            Confidence::Exact,
                            path,
                            "OPC entry inventory",
                        ),
                        Evidence::resolved(
                            "powerpoint.notes_slide_count",
                            json!(notes_count),
                            Confidence::Exact,
                            path,
                            "OPC entry inventory",
                        ),
                    ]);
                }
                "powerpoint.asset_inventory" => {
                    let image_count = session
                        .as_mut()
                        .expect("package session")
                        .unique_image_asset_part_count(context, "ppt/media/")?;
                    let media_count = session
                        .as_mut()
                        .expect("package session")
                        .unique_media_asset_part_count(context, "ppt/media/")?;
                    let names = session.as_ref().expect("package session").entry_names();
                    output.extend([
                        Evidence::resolved(
                            "powerpoint.chart_part_count",
                            json!(count_unique_parts(names, "ppt/charts/chart", ".xml")),
                            Confidence::Exact,
                            path,
                            "unique PowerPoint chart XML parts",
                        ),
                        Evidence::resolved(
                            "powerpoint.unique_image_asset_count",
                            json!(image_count),
                            Confidence::Exact,
                            path,
                            "unique image asset parts under ppt/media/",
                        ),
                        Evidence::resolved(
                            "powerpoint.unique_media_asset_count",
                            json!(media_count),
                            Confidence::Exact,
                            path,
                            "unique audio/video asset parts under ppt/media/",
                        ),
                        Evidence::resolved(
                            "powerpoint.comment_part_count",
                            json!(count_unique_parts(names, "ppt/comments/", ".xml")),
                            Confidence::Exact,
                            path,
                            "unique PowerPoint comment XML parts",
                        ),
                    ]);
                }
                "powerpoint.app_statistics" => {
                    let properties = session
                        .as_mut()
                        .expect("package session")
                        .read_text(context, "docProps/app.xml")?
                        .map(|xml| element_text_map(&xml))
                        .unwrap_or_default();
                    match properties
                        .get("slides")
                        .and_then(|value| value.parse::<u64>().ok())
                    {
                        Some(count) => output.push(Evidence::resolved(
                            "powerpoint.slide_count",
                            json!(count),
                            Confidence::High,
                            path,
                            "docProps/app.xml saved statistic",
                        )),
                        None => output.push(Evidence::unresolved(
                            "powerpoint.slide_count",
                            deckprobe_core::TargetStatus::Unknown,
                            path,
                        )),
                    }
                }
                "powerpoint.presentation_xml" => {
                    let xml = session
                        .as_mut()
                        .expect("package session")
                        .read_text(context, "ppt/presentation.xml")?
                        .ok_or_else(|| {
                            DeckProbeError::MalformedInput(
                                "missing ppt/presentation.xml".to_owned(),
                            )
                        })?;
                    let slides = start_elements_with_attributes(&xml, "sldid");
                    let hidden = slides
                        .iter()
                        .filter(|attrs| {
                            attrs.get("show").is_some_and(|value| {
                                matches!(value.as_str(), "0" | "false" | "off")
                            })
                        })
                        .count();
                    output.push(Evidence::resolved(
                        "powerpoint.slide_count",
                        json!(slides.len()),
                        Confidence::Exact,
                        path,
                        "ppt/presentation.xml",
                    ));
                    output.push(Evidence::resolved(
                        "powerpoint.hidden_slide_count",
                        json!(hidden),
                        Confidence::Exact,
                        path,
                        "ppt/presentation.xml",
                    ));
                    match first_element_attributes(&xml, "sldsz").and_then(parse_slide_size) {
                        Some((width, height)) => {
                            let divisor = gcd(width, height).max(1);
                            let orientation = match width.cmp(&height) {
                                std::cmp::Ordering::Greater => "landscape",
                                std::cmp::Ordering::Less => "portrait",
                                std::cmp::Ordering::Equal => "square",
                            };
                            output.extend([
                                Evidence::resolved(
                                    "powerpoint.slide_size",
                                    json!({
                                        "width_emu": width, "height_emu": height,
                                        "width_pt": width as f64 / 12700.0,
                                        "height_pt": height as f64 / 12700.0
                                    }),
                                    Confidence::Exact,
                                    path,
                                    "ppt/presentation.xml p:sldSz",
                                ),
                                Evidence::resolved(
                                    "powerpoint.aspect_ratio",
                                    json!({
                                        "width": width / divisor, "height": height / divisor,
                                        "decimal": width as f64 / height as f64
                                    }),
                                    Confidence::Exact,
                                    path,
                                    "ppt/presentation.xml p:sldSz",
                                ),
                                Evidence::resolved(
                                    "powerpoint.orientation",
                                    orientation,
                                    Confidence::Exact,
                                    path,
                                    "ppt/presentation.xml p:sldSz",
                                ),
                            ]);
                        }
                        None => {
                            for target in [
                                "powerpoint.slide_size",
                                "powerpoint.aspect_ratio",
                                "powerpoint.orientation",
                            ] {
                                output.push(Evidence::unresolved(
                                    target,
                                    deckprobe_core::TargetStatus::Unknown,
                                    path,
                                ));
                            }
                        }
                    }
                }
                other => {
                    return Err(DeckProbeError::InvalidRequest(format!(
                        "unknown PowerPoint path: {other}"
                    )));
                }
            }
        }
        Ok(output)
    }
}

fn count_parts(names: &[String], prefix: &str, suffix: &str) -> usize {
    names
        .iter()
        .filter(|name| name.starts_with(prefix) && name.ends_with(suffix))
        .count()
}

fn parse_slide_size(attributes: std::collections::BTreeMap<String, String>) -> Option<(u64, u64)> {
    Some((
        attributes.get("cx")?.parse().ok()?,
        attributes.get("cy")?.parse().ok()?,
    ))
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let next = left % right;
        left = right;
        right = next;
    }
    left
}
