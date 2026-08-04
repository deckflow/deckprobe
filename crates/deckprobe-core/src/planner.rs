use std::collections::BTreeSet;

use crate::{DeckProbeError, ExecutionPlan, PathDescriptor, ProbeRequest, Result};

pub fn plan_paths(request: &ProbeRequest, paths: &[PathDescriptor]) -> Result<ExecutionPlan> {
    let mut uncovered = request.targets.clone();
    let mut selected = Vec::new();
    let mut estimated_cost = 0;

    while !uncovered.is_empty() {
        let mut candidates: Vec<(&PathDescriptor, BTreeSet<String>)> = paths
            .iter()
            .filter(|path| path.min_level <= request.level)
            .filter_map(|path| {
                let covered = path
                    .targets
                    .iter()
                    .filter(|target| {
                        uncovered.contains(*target)
                            && path.confidence >= request.minimum_confidence_for(target)
                    })
                    .cloned()
                    .collect::<BTreeSet<_>>();
                (!covered.is_empty()).then_some((path, covered))
            })
            .collect();

        if candidates.is_empty() {
            break;
        }

        candidates.sort_by(|(left, left_covered), (right, right_covered)| {
            let left_score = left.estimated_cost / left_covered.len() as u64;
            let right_score = right.estimated_cost / right_covered.len() as u64;
            left_score
                .cmp(&right_score)
                .then_with(|| right.confidence.cmp(&left.confidence))
                .then_with(|| left.id.cmp(&right.id))
        });

        let (path, covered) = candidates.remove(0);
        uncovered.retain(|target| !covered.contains(target));
        estimated_cost += path.estimated_cost;
        selected.push(path.id.clone());
    }

    let unresolved_targets = uncovered.into_iter().collect::<Vec<_>>();
    if selected.is_empty()
        && !request.targets.is_empty()
        && unresolved_targets.len() == request.targets.len()
    {
        return Err(DeckProbeError::UnsupportedTarget(
            unresolved_targets.join(","),
        ));
    }

    let selected_ids = selected.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let piggyback_targets = if request.allow_piggyback {
        request
            .optional_targets
            .iter()
            .filter(|target| {
                paths.iter().any(|path| {
                    selected_ids.contains(path.id.as_str())
                        && path.min_level <= request.level
                        && path.confidence >= request.minimum_confidence_for(target)
                        && path.targets.contains(target)
                })
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    Ok(ExecutionPlan {
        paths: selected,
        unresolved_targets,
        piggyback_targets,
        estimated_cost,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::{Confidence, ProbeLevel};

    #[test]
    fn planner_prefers_shared_low_cost_path() {
        let request = ProbeRequest {
            targets: BTreeSet::from(["a".to_owned(), "b".to_owned()]),
            optional_targets: BTreeSet::new(),
            level: ProbeLevel::Metadata,
            minimum_confidence: Confidence::High,
            target_confidence: BTreeMap::new(),
            allow_piggyback: true,
            format_options: BTreeMap::new(),
        };
        let paths = vec![
            PathDescriptor::new(
                "single-a",
                &["a"],
                ProbeLevel::Header,
                Confidence::Exact,
                10,
            ),
            PathDescriptor::new(
                "single-b",
                &["b"],
                ProbeLevel::Header,
                Confidence::Exact,
                10,
            ),
            PathDescriptor::new(
                "shared",
                &["a", "b"],
                ProbeLevel::Metadata,
                Confidence::High,
                12,
            ),
        ];
        let plan = plan_paths(&request, &paths).unwrap();
        assert_eq!(plan.paths, vec!["shared"]);
    }

    #[test]
    fn planner_applies_per_target_confidence() {
        let request = ProbeRequest {
            targets: BTreeSet::from(["a".to_owned(), "b".to_owned()]),
            optional_targets: BTreeSet::new(),
            level: ProbeLevel::Metadata,
            minimum_confidence: Confidence::High,
            target_confidence: BTreeMap::from([("b".to_owned(), Confidence::Exact)]),
            allow_piggyback: true,
            format_options: BTreeMap::new(),
        };
        let paths = vec![
            PathDescriptor::new(
                "shared-high",
                &["a", "b"],
                ProbeLevel::Metadata,
                Confidence::High,
                2,
            ),
            PathDescriptor::new(
                "b-exact",
                &["b"],
                ProbeLevel::Metadata,
                Confidence::Exact,
                2,
            ),
        ];
        let plan = plan_paths(&request, &paths).unwrap();
        assert!(plan.paths.contains(&"shared-high".to_owned()));
        assert!(plan.paths.contains(&"b-exact".to_owned()));
        assert!(plan.unresolved_targets.is_empty());
    }

    #[test]
    fn planner_only_piggybacks_targets_from_selected_paths() {
        let request = ProbeRequest {
            targets: BTreeSet::from(["a".to_owned()]),
            optional_targets: BTreeSet::from(["b".to_owned(), "c".to_owned()]),
            level: ProbeLevel::Metadata,
            minimum_confidence: Confidence::High,
            target_confidence: BTreeMap::new(),
            allow_piggyback: true,
            format_options: BTreeMap::new(),
        };
        let paths = vec![
            PathDescriptor::new(
                "selected",
                &["a", "b"],
                ProbeLevel::Metadata,
                Confidence::High,
                2,
            ),
            PathDescriptor::new("extra", &["c"], ProbeLevel::Metadata, Confidence::Exact, 1),
        ];
        let plan = plan_paths(&request, &paths).unwrap();
        assert_eq!(plan.paths, vec!["selected"]);
        assert_eq!(plan.piggyback_targets, vec!["b"]);
    }
}
