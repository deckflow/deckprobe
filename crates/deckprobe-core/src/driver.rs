use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use crate::{
    Confidence, Evidence, ExecutionPlan, FormatProfile, OptionSpec, PathDescriptor, ProbeContext,
    ProbeLevel, ProbeRequest, Result, TargetSpec,
};

pub trait FormatDriver {
    fn id(&self) -> &'static str;
    fn profile(&self) -> &FormatProfile;
    fn targets(&self) -> Vec<TargetSpec>;
    fn options(&self) -> Vec<OptionSpec>;
    fn default_targets(&self, level: crate::ProbeLevel) -> BTreeSet<String>;

    /// Targets for which this driver declares at least one executable path at
    /// the requested level. The common target catalog is intentionally broader
    /// than any one format, so selectors must use path applicability instead of
    /// treating every catalog entry as implemented.
    fn applicable_targets(&self, level: ProbeLevel) -> BTreeSet<String> {
        let candidates = self
            .targets()
            .into_iter()
            .filter(|spec| spec.min_level <= level)
            .map(|spec| spec.id)
            .collect::<BTreeSet<_>>();
        let request = ProbeRequest {
            targets: candidates.clone(),
            optional_targets: BTreeSet::new(),
            level,
            minimum_confidence: Confidence::None,
            target_confidence: BTreeMap::new(),
            allow_piggyback: true,
            format_options: BTreeMap::new(),
        };
        self.paths(&request)
            .map(|paths| {
                paths
                    .into_iter()
                    .filter(|path| path.min_level <= level)
                    .flat_map(|path| path.targets)
                    .filter(|target| candidates.contains(target))
                    .collect()
            })
            .unwrap_or_else(|_| self.default_targets(level))
    }

    fn paths(&self, request: &ProbeRequest) -> Result<Vec<PathDescriptor>>;
    fn validate_options(&self, request: &ProbeRequest) -> Result<()>;
    fn execute(
        &self,
        context: &mut ProbeContext,
        request: &ProbeRequest,
        plan: &ExecutionPlan,
    ) -> Result<Vec<Evidence>>;
}

pub fn identity_evidence(
    context: &ProbeContext,
    profile: &FormatProfile,
    path: &str,
) -> Vec<Evidence> {
    let extension = context.extension();
    let extension_matches = extension
        .as_deref()
        .is_some_and(|value| profile.extensions.contains(&value));

    vec![
        Evidence::resolved(
            "document.format",
            profile.format,
            crate::Confidence::Exact,
            path,
            "magic/container",
        ),
        Evidence::resolved(
            "document.format_profile",
            profile.profile,
            crate::Confidence::Exact,
            path,
            "magic/container",
        ),
        Evidence::resolved(
            "document.mime_type",
            profile.mime_type,
            crate::Confidence::Exact,
            path,
            "format profile",
        ),
        Evidence::resolved(
            "document.file_size",
            json!(context.file_size()),
            crate::Confidence::Exact,
            path,
            if context.source_kind() == "local_file" {
                "filesystem metadata"
            } else {
                "source length"
            },
        ),
        Evidence::resolved(
            "document.extension",
            extension.unwrap_or_default(),
            crate::Confidence::Exact,
            path,
            if context.source_kind() == "local_file" {
                "input path"
            } else {
                "input name"
            },
        ),
        Evidence::resolved(
            "document.extension_matches",
            extension_matches,
            crate::Confidence::Exact,
            path,
            if context.source_kind() == "local_file" {
                "input path + detected profile"
            } else {
                "input name + detected profile"
            },
        ),
    ]
}
