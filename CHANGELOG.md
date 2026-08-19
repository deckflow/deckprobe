# Changelog

All notable changes to DeckProbe are documented here.

## [Unreleased]

## [2.4.0] - 2026-08-19

### Added

- DeckProbe ships an Agent Skill at `skills/deckprobe/`, teaching coding agents
  the target vocabulary, the report contract, and exit-status handling, with
  progressive-disclosure references for targets, recipes, output, and limits.
- New `deckprobe install` subcommand writes support files into conventional local
  directories and reports one JSON receipt. `--skills` (the default artifact set)
  installs the agent skill for `--agent`-selected agents or an explicit `--dir`;
  `--man` and `--completions` install the generated manual pages and shell
  completion source. Every artifact shares the same idempotent
  created/updated/unchanged policy, `--force` gate, and `--dry-run`.
- `.claude-plugin/` manifests publish the repository as a single-plugin Claude
  Code marketplace, so `/plugin marketplace add deckflow/deckprobe` works
  alongside `npx skills add deckflow/deckprobe`. All three install routes deliver
  identical bytes, verified by a contract test.

### Changed

- The subcommand section of `deckprobe --help` is now headed `Commands` rather
  than `Discovery commands`, because `install` is not a discovery command.

## [2.3.1] - 2026-08-11

### Fixed

- Publish a complete `@deckflow/deckprobe` tarball containing the compiled
  `dist/` JavaScript and `wasm/` runtime artifacts. The release gate now rejects
  an npm package when any required build artifact is absent.

## [2.3.0] - 2026-08-06

### Added

- Node.js is a first-class target for `@deckflow/deckprobe`: the `node` export
  condition loads WASM from disk, `probe()` initializes lazily without a custom
  fetch, and `probeFile(path)` returns reports whose `source_kind` matches the
  native CLI for the same file.
- The npm package ships the native `deckprobe` CLI on PATH via per-platform
  optional dependencies. Release CI repackages the same cargo-dist archives that
  GitHub Releases serve, so `npx @deckflow/deckprobe` and the shell installers
  hand out byte-identical binaries.

### Fixed

- Browser and native WASM paths now report identical `confidence_score` values
  for the same probe.
- The browser SDK exposes a stable WASM entry and guards initialization so
  bundlers and Workers resolve the module reliably.
- Release CI coverage for the `--artifacts` platform-package packaging path
  (archive discovery, tar/zip extraction, and generated manifests).

## [2.2.1] - 2026-08-04

### Fixed

- Include WASM artifacts in the published `@deckflow/deckprobe` package. `wasm-pack`
  emits `wasm/.gitignore` with `*`, which caused `npm pack` to drop the entire
  `wasm/` directory; the build now removes that file before packing.

## [2.2.0] - 2026-08-03

### Added

- Reproducible Chromium smoke coverage for all browser input forms, Worker
  concurrency/lifecycle, structured errors, discovery, and schema validation.
- npm package build, browser-test, dry-pack, promotion, and trusted-publishing
  release gates.
- Modern iWork producer build, language/locale, external-or-missing-data,
  Data/ byte/type inventory, preview-dimension, and full-IWA integrity targets.
- Target discovery now reports per-format applicability and supported probe
  levels; the TypeScript SDK exposes these fields through `TargetSpecReport`.

### Changed

- Worker probes now move `Blob`/`File` reads into the Worker and preserve
  JavaScript error classes across the Worker boundary.
- Invalid WASM options reject with `TypeError`, while failed WASM
  initialization can be retried.

### Fixed

- Reject probes immediately after a Worker is failed or terminated instead of
  leaving their promises pending forever.
- Accept source-independent `source_kind` values in the schema-v2 contract.
- Include the browser SDK source in the isolated release promotion allowlist.
- Preserve `BUDGET_EXCEEDED` when ZIP parsers wrap bounded-reader failures as a
  generic I/O error.
- Filter scenario selectors through each driver's executable paths so iWork
  `@summary`, `@security`, `@quality`, and `@all` do not request phantom common
  targets.
- Read iWork producer builds from `BuildVersionHistory.plist` instead of
  presenting `fileFormatVersion` as the application version.

## [2.0.0] - 2026-08-03

### Added

- Source-independent `deckprobe-engine` crate for driver dispatch, target
  expansion, planning, execution, discovery, error mapping, and schema-v2 report
  assembly.
- Re-openable `ProbeSource`/`ProbeReader`, in-memory sources, serializable
  `ProbeOptions`, budget overrides, and browser-compatible timeout accounting.
- Raw stdin probing with `--stdin-name` and JSONL multi-input processing for
  local paths or named base64 byte payloads.
- `deckprobe-wasm` browser binding accepting logical filename, `Uint8Array`, and
  JavaScript options.
- Independently publishable `@deckflow/deckprobe` TypeScript package with
  `File`, `Blob`, `ArrayBuffer`, `Uint8Array`, discovery, and transferable Web
  Worker APIs.

### Changed

- Format crates now consume only budgeted source readers and no longer open
  concrete files.
- The native CLI is a thin path/stdin/JSONL adapter over the shared engine and
  no longer depends directly on any format crate.
- The Rust workspace version moves to 2.0 because the library-facing crate
  boundaries and source API are new, while the CLI report schema remains v2.

### Compatibility

- Existing 1.1 CLI flags, selectors, stable target IDs, full/values reports,
  errors, and discovery contracts remain supported.
- Browser and native adapters return the same schema-v2 reports from the same
  engine.

## [1.1.0] - 2026-08-02

### Added

- Additive CLI aliases for probe level, confidence, driver options, pretty
  output, strict mode, and plan-only mode, including compact level/confidence
  values and repeatable target selection.
- Unambiguous short target names plus `@summary`, `@security`, `@structure`,
  `@assets`, and `@quality` selector presets.
- Compact `--view values` output, positional target discovery, machine-readable
  target schemas/aliases/cost classes/selector expansions, bundled schema
  discovery, and generated shell completions.
- Per-target confidence overrides and zero-additional-path optional target
  piggybacking with explicit report attribution.
- PDF security, form, annotation, attachment, and XMP targets; OOXML security
  inventory and Word/Excel/PowerPoint asset-part summary targets; iWork preview
  count and readable-package password signal.
- A 1.1 target-shape benchmark and deterministic iWork I/O-cost contracts.

### Changed

- Modern iWork dispatch now transfers its live validated ZIP session to the
  selected driver, avoiding a second central-directory scan.
- Target discovery retains the compatibility `value_type` while adding a JSON
  Schema fragment suitable for generated callers.

### Compatibility

- Existing long options, stable target IDs, schema-v2 full reports, and driver
  default target sets remain supported. New report fields are omitted when
  unused.

## [1.0.0-beta.1] - 2026-08-02

### Added

- Modern Apple Keynote (`.key`), Numbers (`.numbers`), and Pages (`.pages`) drivers.
- Shared bounded iWork ZIP, binary/XML plist, IWA framing, raw Snappy block, and Protobuf wire decoder.
- Keynote slide/master/table component inventory, Numbers ordered sheet names/count, and Pages table component inventory.
- Three local iWork fixtures and synthetic release-gate coverage while retaining the 100-case release contract.

### Changed

- Project lifecycle moves from MVP to the 1.0 beta series.
- Header budgets now accommodate realistic modern iWork central directories and bounded `Document.iwa` validation.
- Format discovery, CLI help, architecture documentation, and fixture policy include Apple iWork.

### Unsupported

- Legacy XML iWork packages (`index.apxl` or `index.xml`) are recognized and return `UNSUPPORTED_FORMAT`; they are not parsed or silently treated as malformed modern IWA.

## [0.2.0] - 2026-08-02

### Added

- Agent-friendly schema-v2 JSON envelopes for successful, partial, and failed commands, with stable error codes and tool versioning.
- Deterministic default reports; optional wall-clock telemetry via `--telemetry`.
- Bounded safe PDF xref normalization and reconstruction, exposed through `pdf.repair_xref` and `pdf.repaired`.
- Legacy DOC/XLS/PPT CFB validation, SummaryInformation metadata, macro inventory, and core format statistics using the same targets as OOXML.
- Tracked JSON Schema and clean-tree integration coverage for deterministic output, routing, budgets, confidence, and structured errors.

### Changed

- Filename extensions now select the driver path; container signatures and internal Office types are validated and mismatches fail explicitly.
- OOXML identity detection now runs inside physical-byte, archive-entry, expanded-byte, and timeout budgets.
- OOXML entry inventory no longer opens every ZIP member, substantially reducing random reads.
- `--minimum-confidence` is enforced against returned evidence as well as planned paths.
- Minimum supported Rust version is now 1.88.

## [0.1.0] - 2026-08-02

### Added

- Target-driven Rust CLI and JSON report schema MVP.
- Shared Source, Budget, Evidence, Path, and Planner contracts.
- PDF, Word, Excel, PowerPoint, and legacy Office format drivers.
- Shared budgeted OOXML ZIP/OPC/XML paths.
- Format-specific targets, options, and confidence-aware path selection.
- Local fixture integrity checks and CLI route regression tests.
