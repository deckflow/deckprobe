//! Installable DeckProbe assets and the file-materialization policy behind
//! `deckprobe install`.
//!
//! The agent skill is embedded from `skills/deckprobe/` at compile time, the same
//! way the engine embeds the report schema from `docs/`. That makes the repository
//! tree the single source of truth for every distribution channel: `deckprobe
//! install --skills`, `npx skills add deckflow/deckprobe`, and the Claude Code
//! plugin all deliver identical bytes. It also means a release tree that forgot to
//! ship `skills/` fails to compile instead of publishing a broken installer.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use deckprobe_core::{DeckProbeError, Result};
use serde_json::{Value, json};

/// Directory name the skill is installed under, and its `name:` in frontmatter.
pub const SKILL_NAME: &str = "deckprobe";

/// Frontmatter key that marks an installed skill directory as DeckProbe-owned.
///
/// Its presence in an existing `SKILL.md` is what lets an upgrade overwrite the
/// directory without `--force`, while still refusing to clobber a skill somebody
/// else wrote under the same name.
const SKILL_OWNERSHIP_MARKER: &str = "deckprobe-skill-format:";

/// Every file under `skills/deckprobe/`, embedded.
///
/// The `embedded_skill` tests below fail if this table and the directory ever
/// disagree.
pub const SKILL_FILES: &[(&str, &str)] = &[
    (
        "SKILL.md",
        include_str!("../../../skills/deckprobe/SKILL.md"),
    ),
    (
        "references/targets.md",
        include_str!("../../../skills/deckprobe/references/targets.md"),
    ),
    (
        "references/recipes.md",
        include_str!("../../../skills/deckprobe/references/recipes.md"),
    ),
    (
        "references/output.md",
        include_str!("../../../skills/deckprobe/references/output.md"),
    ),
    (
        "references/limits.md",
        include_str!("../../../skills/deckprobe/references/limits.md"),
    ),
];

/// Agent whose conventional skills directory receives an installed skill.
///
/// DeckProbe deliberately tracks only the agents with a distinct, documented
/// install base plus the vendor-neutral `.agents/skills` layout that covers the
/// long tail. Anything else is served by `--dir`, or by `npx skills add`, which
/// maintains a far larger table upstream.
#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentTarget {
    /// Every agent already present at the chosen scope; falls back to `agents`.
    Auto,
    /// Every agent in the table.
    All,
    #[value(alias = "claude-code")]
    Claude,
    #[value(alias = "codex-cli")]
    Codex,
    Cursor,
    Opencode,
    #[value(alias = "gemini-cli")]
    Gemini,
    #[value(alias = "github-copilot")]
    Copilot,
    Windsurf,
    Cline,
    Zed,
    /// The vendor-neutral `.agents/skills` layout.
    #[value(alias = "universal")]
    Agents,
}

/// `(agent, project-relative directory, home-relative directory)`.
const AGENT_PATHS: &[(AgentTarget, &str, &str)] = &[
    (AgentTarget::Claude, ".claude/skills", ".claude/skills"),
    (AgentTarget::Codex, ".agents/skills", ".codex/skills"),
    (AgentTarget::Cursor, ".agents/skills", ".cursor/skills"),
    (
        AgentTarget::Opencode,
        ".agents/skills",
        ".config/opencode/skills",
    ),
    (AgentTarget::Gemini, ".agents/skills", ".gemini/skills"),
    (AgentTarget::Copilot, ".agents/skills", ".copilot/skills"),
    (
        AgentTarget::Windsurf,
        ".windsurf/skills",
        ".codeium/windsurf/skills",
    ),
    (AgentTarget::Cline, ".agents/skills", ".agents/skills"),
    (AgentTarget::Zed, ".agents/skills", ".agents/skills"),
    (AgentTarget::Agents, ".agents/skills", ".agents/skills"),
];

impl AgentTarget {
    /// Stable identifier used in the JSON receipt.
    ///
    /// Kept in sync with the `ValueEnum` names by `agent_ids_match_the_clap_names`.
    fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::All => "all",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Opencode => "opencode",
            Self::Gemini => "gemini",
            Self::Copilot => "copilot",
            Self::Windsurf => "windsurf",
            Self::Cline => "cline",
            Self::Zed => "zed",
            Self::Agents => "agents",
        }
    }

    fn directory(self, global: bool) -> Option<&'static str> {
        AGENT_PATHS
            .iter()
            .find(|(agent, _, _)| *agent == self)
            .map(|(_, project, home)| if global { *home } else { *project })
    }
}

/// One file DeckProbe intends to write.
pub struct PlannedFile {
    /// Path relative to the artifact's destination directory.
    pub path: String,
    pub contents: Vec<u8>,
}

/// Everything `deckprobe install` needs to resolve destinations and write files.
pub struct InstallRequest<'a> {
    pub skills: bool,
    pub man: bool,
    pub completions: Option<clap_complete::Shell>,
    pub agents: &'a [AgentTarget],
    pub global: bool,
    pub dir: Option<&'a Path>,
    pub force: bool,
    pub dry_run: bool,
}

impl InstallRequest<'_> {
    fn artifact_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.skills {
            names.push("skills");
        }
        if self.man {
            names.push("man");
        }
        if self.completions.is_some() {
            names.push("completions");
        }
        names
    }

    fn scope(&self) -> &'static str {
        if self.dir.is_some() {
            "explicit"
        } else if self.global {
            "global"
        } else {
            "project"
        }
    }
}

/// Resolve the home directory without depending on a platform crate.
fn home_directory() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|home| !home.as_os_str().is_empty())
        .ok_or_else(|| {
            DeckProbeError::InvalidRequest(
                "cannot resolve the home directory; set HOME or use --dir".to_owned(),
            )
        })
}

/// Skills-container directories to install into, each with the agents it serves.
///
/// Several agents share `.agents/skills` at project scope, so destinations are
/// deduplicated and every agent that resolved to one is reported against it.
fn resolve_skill_destinations(
    request: &InstallRequest<'_>,
) -> Result<Vec<(PathBuf, Vec<&'static str>)>> {
    if let Some(directory) = request.dir {
        return Ok(vec![(directory.to_path_buf(), Vec::new())]);
    }

    let root = if request.global {
        home_directory()?
    } else {
        PathBuf::from(".")
    };

    let requested: BTreeSet<AgentTarget> = if request.agents.is_empty() {
        BTreeSet::from([AgentTarget::Auto])
    } else {
        request.agents.iter().copied().collect()
    };

    let selected: Vec<AgentTarget> = if requested.contains(&AgentTarget::All) {
        AGENT_PATHS.iter().map(|(agent, _, _)| *agent).collect()
    } else if requested.contains(&AgentTarget::Auto) {
        let present: Vec<AgentTarget> = AGENT_PATHS
            .iter()
            .filter(|(agent, _, _)| {
                agent
                    .directory(request.global)
                    .and_then(|relative| Path::new(relative).parent().map(|p| root.join(p)))
                    .is_some_and(|marker| marker.is_dir())
            })
            .map(|(agent, _, _)| *agent)
            .collect();
        if present.is_empty() {
            vec![AgentTarget::Agents]
        } else {
            present
        }
    } else {
        requested.into_iter().collect()
    };

    let mut destinations: BTreeMap<PathBuf, Vec<&'static str>> = BTreeMap::new();
    for agent in selected {
        let Some(relative) = agent.directory(request.global) else {
            continue;
        };
        destinations
            .entry(root.join(relative))
            .or_default()
            .push(agent.id());
    }
    Ok(destinations.into_iter().collect())
}

/// Refuse to replace a skill directory somebody else wrote.
///
/// Ownership is established by the marker in `SKILL.md`. This runs as a preflight
/// across every resolved destination before any of them is written, so a run that
/// touches several agents either writes all of them or none.
fn check_skill_ownership(directory: &Path, request: &InstallRequest<'_>) -> Result<()> {
    if request.force {
        return Ok(());
    }
    let manifest = directory.join("SKILL.md");
    if let Ok(existing) = std::fs::read_to_string(&manifest)
        && !existing.contains(SKILL_OWNERSHIP_MARKER)
    {
        return Err(DeckProbeError::InvalidRequest(format!(
            "{} exists and was not written by deckprobe; rerun with --force to replace it, \
             or use --agent/--dir to install somewhere else",
            manifest.display()
        )));
    }
    Ok(())
}

/// Write one artifact's files into `directory`, reporting what changed.
fn materialize(
    directory: &Path,
    files: &[PlannedFile],
    request: &InstallRequest<'_>,
) -> Result<(Vec<Value>, Vec<String>)> {
    let mut written = Vec::new();
    for file in files {
        let path = directory.join(&file.path);
        let action = match std::fs::read(&path) {
            Ok(existing) if existing == file.contents => "unchanged",
            Ok(_) => "updated",
            Err(_) => "created",
        };
        if !request.dry_run && action != "unchanged" {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &file.contents)?;
        }
        written.push(json!({
            "path": file.path,
            "bytes": file.contents.len(),
            "action": action,
        }));
    }

    Ok((written, orphaned_files(directory, files)))
}

/// Files already in the destination that this version no longer ships.
///
/// Reported so an upgrade is visible, never deleted: the directory may hold notes
/// or local additions that are not DeckProbe's to remove.
fn orphaned_files(directory: &Path, files: &[PlannedFile]) -> Vec<String> {
    let expected: BTreeSet<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let mut found = Vec::new();
    collect_relative_files(directory, "", &mut found);
    found.retain(|path| !expected.contains(path.as_str()));
    found.sort();
    found
}

fn collect_relative_files(directory: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => {
                collect_relative_files(&entry.path(), &relative, out);
            }
            Ok(_) => out.push(relative),
            Err(_) => {}
        }
    }
}

/// The embedded skill as writable files.
fn skill_payload() -> Vec<PlannedFile> {
    SKILL_FILES
        .iter()
        .map(|(path, contents)| PlannedFile {
            path: (*path).to_owned(),
            contents: contents.as_bytes().to_vec(),
        })
        .collect()
}

/// Execute an install request and return the JSON receipt.
///
/// `man_pages` and `completion_script` are supplied by the caller because both are
/// rendered from the live clap command model, which lives in `main`.
pub fn install(
    request: &InstallRequest<'_>,
    man_pages: impl FnOnce() -> Result<Vec<PlannedFile>>,
    completion_script: impl FnOnce(clap_complete::Shell) -> PlannedFile,
) -> Result<Value> {
    // Resolve and validate every destination before writing anything, so a
    // request that is going to be rejected cannot leave a half-installed tree
    // behind -- across artifacts as well as across agents.
    let skill_destinations: Vec<(PathBuf, Vec<&'static str>)> = if request.skills {
        let destinations: Vec<(PathBuf, Vec<&'static str>)> = resolve_skill_destinations(request)?
            .into_iter()
            .map(|(container, agents)| (container.join(SKILL_NAME), agents))
            .collect();
        for (directory, _) in &destinations {
            check_skill_ownership(directory, request)?;
        }
        destinations
    } else {
        Vec::new()
    };

    let man_directory = if request.man {
        Some(man_destination(request)?)
    } else {
        None
    };

    let completions_directory = match request.completions {
        Some(_) => Some(request.dir.ok_or_else(|| {
            DeckProbeError::InvalidRequest(
                "--completions requires --dir; shell completion directories are not standardized"
                    .to_owned(),
            )
        })?),
        None => None,
    };

    let mut targets = Vec::new();

    if request.skills {
        let payload = skill_payload();
        for (directory, agents) in skill_destinations {
            let (files, orphaned) = materialize(&directory, &payload, request)?;
            targets.push(json!({
                "artifact": "skills",
                "name": SKILL_NAME,
                "agents": agents,
                "directory": directory,
                "files": files,
                "orphaned": orphaned,
            }));
        }
    }

    if let Some(directory) = man_directory {
        let (files, _) = materialize(&directory, &man_pages()?, request)?;
        targets.push(json!({
            "artifact": "man",
            "directory": directory,
            "files": files,
        }));
    }

    if let (Some(shell), Some(directory)) = (request.completions, completions_directory) {
        let (files, _) = materialize(directory, &[completion_script(shell)], request)?;
        targets.push(json!({
            "artifact": "completions",
            "shell": shell.to_string(),
            "directory": directory,
            "files": files,
        }));
    }

    Ok(json!({
        "schema_version": 2,
        "tool_version": env!("CARGO_PKG_VERSION"),
        "status": "ok",
        "install": {
            "artifacts": request.artifact_names(),
            "scope": request.scope(),
            "dry_run": request.dry_run,
            "force": request.force,
            "targets": targets,
        }
    }))
}

fn man_destination(request: &InstallRequest<'_>) -> Result<PathBuf> {
    if let Some(directory) = request.dir {
        return Ok(directory.to_path_buf());
    }
    if !request.global {
        return Ok(PathBuf::from("man"));
    }
    if let Some(share) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(share).join("man/man1"));
    }
    Ok(home_directory()?.join(".local/share/man/man1"))
}

#[cfg(test)]
mod embedded_skill {
    use super::{AGENT_PATHS, AgentTarget, SKILL_FILES, SKILL_NAME, SKILL_OWNERSHIP_MARKER};
    use clap::ValueEnum;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// Fields the Agent Skills spec allows. Anything else is a hard error on
    /// claude.ai upload, the Skills API, and `package_skill.py`, so it must never
    /// reach the checked-in `SKILL.md` even though Claude Code itself accepts more.
    const SPEC_FRONTMATTER_FIELDS: &[&str] = &[
        "name",
        "description",
        "license",
        "compatibility",
        "metadata",
        "allowed-tools",
    ];

    fn skill_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../skills")
            .join(SKILL_NAME)
    }

    fn walk(directory: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(directory).expect("skills/deckprobe is readable") {
            let entry = entry.expect("directory entry is readable");
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type().expect("file type is readable").is_dir() {
                walk(&entry.path(), &relative, out);
            } else {
                out.push(relative);
            }
        }
    }

    fn skill_manifest() -> &'static str {
        SKILL_FILES
            .iter()
            .find(|(path, _)| *path == "SKILL.md")
            .expect("the skill always ships a SKILL.md")
            .1
    }

    /// Top-level frontmatter keys, without pulling in a YAML dependency.
    fn frontmatter_keys(manifest: &str) -> Vec<String> {
        let body = manifest
            .strip_prefix("---\n")
            .expect("SKILL.md opens with a frontmatter fence");
        let (frontmatter, _) = body
            .split_once("\n---\n")
            .expect("SKILL.md closes its frontmatter fence");
        frontmatter
            .lines()
            .filter(|line| !line.starts_with(char::is_whitespace) && !line.trim().is_empty())
            .filter_map(|line| line.split_once(':'))
            .map(|(key, _)| key.trim().to_owned())
            .collect()
    }

    #[test]
    fn every_repository_skill_file_is_embedded() {
        let mut on_disk = Vec::new();
        walk(&skill_root(), "", &mut on_disk);
        on_disk.sort();
        let mut embedded: Vec<String> = SKILL_FILES
            .iter()
            .map(|(path, _)| (*path).to_owned())
            .collect();
        embedded.sort();
        assert_eq!(
            on_disk, embedded,
            "skills/{SKILL_NAME}/ and SKILL_FILES disagree; update the include_str! table"
        );
    }

    #[test]
    fn skill_frontmatter_uses_only_agent_skills_spec_fields() {
        let keys = frontmatter_keys(skill_manifest());
        for key in &keys {
            assert!(
                SPEC_FRONTMATTER_FIELDS.contains(&key.as_str()),
                "frontmatter key {key:?} is outside the Agent Skills spec and breaks \
                 claude.ai upload and the Skills API; allowed: {SPEC_FRONTMATTER_FIELDS:?}"
            );
        }
        assert!(keys.iter().any(|key| key == "name"));
        assert!(keys.iter().any(|key| key == "description"));
    }

    #[test]
    fn skill_declares_its_name_and_stays_within_the_listing_budget() {
        let manifest = skill_manifest();
        assert!(
            manifest.contains(&format!("name: {SKILL_NAME}")),
            "SKILL.md must declare name: {SKILL_NAME}"
        );
        assert!(
            manifest.contains(SKILL_OWNERSHIP_MARKER),
            "SKILL.md must carry the {SKILL_OWNERSHIP_MARKER} ownership marker so an \
             upgrade can replace an installed copy without --force"
        );

        let frontmatter = manifest
            .strip_prefix("---\n")
            .and_then(|body| body.split_once("\n---\n"))
            .expect("SKILL.md has frontmatter")
            .0;
        let description = frontmatter
            .split_once("description:")
            .expect("SKILL.md declares a description")
            .1;
        let description = description
            .split("\nlicense:")
            .next()
            .expect("description is followed by another field");
        assert!(
            description.len() < 1_536,
            "description is {} bytes; the skill listing truncates at 1536",
            description.len()
        );
    }

    #[test]
    fn every_agent_has_exactly_one_path_entry() {
        let mut seen = BTreeSet::new();
        for (agent, project, home) in AGENT_PATHS {
            assert!(
                seen.insert(*agent),
                "{agent:?} appears twice in AGENT_PATHS"
            );
            for relative in [project, home] {
                assert!(
                    Path::new(relative)
                        .parent()
                        .is_some_and(|p| !p.as_os_str().is_empty()),
                    "{relative} must nest under a marker directory so --agent auto can detect it"
                );
            }
        }
        for agent in [AgentTarget::Auto, AgentTarget::All] {
            assert!(
                !seen.contains(&agent),
                "{agent:?} is a selector, not a destination, and must stay out of AGENT_PATHS"
            );
        }
        assert!(
            seen.contains(&AgentTarget::Agents),
            "the vendor-neutral fallback must exist"
        );
    }

    #[test]
    fn agent_ids_match_the_clap_names() {
        for agent in AgentTarget::value_variants() {
            let name = agent
                .to_possible_value()
                .expect("every AgentTarget variant is selectable");
            assert_eq!(
                agent.id(),
                name.get_name(),
                "AgentTarget::id and the ValueEnum name have drifted"
            );
        }
    }
}
