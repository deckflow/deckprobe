<div align="center">

# DeckProbe

**`ffprobe` for documents, built for agents.**

Ask for the facts you need. Get structured JSON with confidence, evidence, and measured I/O cost.

[![CI](https://github.com/deckflow/deckprobe/actions/workflows/ci.yml/badge.svg)](https://github.com/deckflow/deckprobe/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-2f80ed.svg)](LICENSE)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org/)

[Install](docs/INSTALLATION.md) · [Quickstart](#quickstart) · [Runtime role](#where-deckprobe-fits) · [Agent skill](#use-from-a-coding-agent) · [MCP server](#mcp-server) · [npm package](#javascript-package) · [Execution modes](#execution-modes) · [Examples](#common-recipes) · [Formats](#supported-formats) · [CLI reference](docs/CLI-REFERENCE.md)

</div>

DeckProbe is the probing engine of the DeckFlow Runtime. It performs low-cost
preflight inspection and returns structured evidence that callers can use before
more expensive parsing, OCR, rendering, uploading, or model ingestion.

Available as a target-driven Rust engine, native CLI, and browser SDK, DeckProbe
inspects untrusted PDF, Microsoft Office, and modern Apple iWork documents
without rendering them or starting a desktop office suite. Instead of eagerly
unpacking everything, it chooses the cheapest probe path that can satisfy the
targets and confidence you requested.

```console
$ deckprobe --pretty -t slide_count deck.pptx
{
  "driver": { "id": "powerpoint", "profile": "pptx" },
  "results": {
    "<target>": {
      "status": "resolved",
      "value": 31,
      "confidence": "high",
      "path": "<selected-path>"
    }
  },
  ...
}
```

User-facing examples use short target names; reports retain stable canonical
keys for machine consumers.

## Where DeckProbe fits

DeckFlow is the Document Runtime for Agents. Its product model has three engine
responsibilities:

| Engine | Responsibility |
|---|---|
| Probe / DeckProbe | Inspect and expose bounded preflight signals for routing decisions. |
| Parse / DeckParse | Make document content and structure operable as persistent state. |
| Render / DeckRender | Produce visual representations for humans, AI, and software. |

DeckProbe can run independently. In a larger Runtime workflow, its report helps
the calling agent or application decide whether to parse, render, reject, or use
another path. DeckProbe provides the evidence for that decision; it does not
automatically choose or execute downstream actions.

DeckProbe is not a general-purpose parser, renderer, OCR engine, editor, or
complete security product.

## Why DeckProbe

| Need | What DeckProbe does |
|---|---|
| Fast, focused inspection | Runs only the paths needed for targets such as page count or slide count. |
| Agent-friendly automation | Emits deterministic schema-v2 JSON for success, partial results, and errors, with stable codes and target-level evidence. |
| Predictable work | Enforces physical-read, decompression, archive-entry, and wall-clock budgets. |
| Broad PDF compatibility | Uses normal parsing first, then bounded safe xref normalization/reconstruction for common damaged-but-readable PDFs. |
| Modern and Legacy Office | Reads OOXML plus `.doc`, `.xls`, and `.ppt` metadata and core statistics through the same target vocabulary. |
| Modern Apple iWork | Validates and inspects `.key`, `.numbers`, and `.pages` ZIP/IWA packages through bounded Snappy and Protobuf paths. |
| Format safety | Routes by filename extension, then verifies the container and required internal main part before reporting values. |

The native CLI needs no Python, JVM, Microsoft Office installation, or external
PDF library at runtime. The browser SDK runs the same engine locally through
WebAssembly and does not upload document bytes.

## Quickstart

### Install from source

Requires [Rust 1.88 or newer](https://www.rust-lang.org/tools/install) and Git.
This builds the CLI on the user's machine and installs it in the Cargo user
directory, so it normally does not require administrator privileges:

```sh
cargo install --git https://github.com/deckflow/deckprobe --locked deckprobe
```

### Install a prebuilt release

The installers choose the matching CPU and platform archive and install into
the Cargo user directory. Installing there does not require administrator
privileges. Use an elevated shell only when deliberately installing into a
system directory.

#### macOS (Apple Silicon or Intel)

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/deckflow/deckprobe/releases/latest/download/deckprobe-installer.sh | sh
```

#### Linux (x86-64 or ARM64; GNU or musl)

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/deckflow/deckprobe/releases/latest/download/deckprobe-installer.sh | sh
```

#### Windows (x86-64, MSVC)

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/deckflow/deckprobe/releases/latest/download/deckprobe-installer.ps1 | iex"
```

If the PowerShell installer updates the user `Path`, restart the shell before
running `deckprobe`.

Every release contains platform archives, per-archive SHA-256 files, a
consolidated checksum file, and GitHub build-provenance attestations. The shell
installer verifies its embedded checksum when `sha256sum` is available. For a
manual verification, download the matching archive and checksum from the same
GitHub Release and use `shasum -a 256` on macOS, `sha256sum` on Linux, or
`Get-FileHash -Algorithm SHA256` on Windows.

The checksum protects against accidental corruption and the provenance
attestation records how the artifact was built. They are not the same as an OS
code signature: this project does not currently publish macOS Developer ID
notarization or Windows Authenticode signatures. macOS Gatekeeper or Windows
SmartScreen may therefore ask for an explicit confirmation for a downloaded
binary. A source-built binary is also not project-signed, but a locally built
executable normally does not carry the quarantine marker attached to browser
downloads.

The one-line installer commands are a convenience for trusted environments.
For a controlled or offline installation, download the installer and archive
from the GitHub Release, inspect them locally, verify the checksum and (where
required) the build-provenance attestation, then run the installer.

At runtime DeckProbe does not need root or administrator access. It needs
execute permission on the binary and read permission on the document being
inspected; output is written to standard output. Hardened environments can
still impose additional `noexec`, SELinux/AppArmor, or enterprise execution
policies.

See the [installation guide](docs/INSTALLATION.md) for custom install locations, upgrades, and uninstallation.

Verify the installation and probe a file:

```sh
deckprobe --version
deckprobe --pretty report.pdf
```

The default `metadata` probe returns common metadata plus useful format-specific facts. For a focused query, name one or more targets:

```sh
deckprobe --pretty \
  -t format,slide_count \
  deck.pptx
```

Target options are repeatable and unambiguous short names are resolved after
format detection:

```sh
deckprobe -l m -t title -t slide_count -p deck.pptx
deckprobe -t @summary,@security --view values deck.pptx
```

Optional zero-additional-path values and per-target confidence are explicit:

```sh
deckprobe -t slide_count -o orientation,aspect_ratio \
  -C slide_count=x deck.pptx
```

Probe one raw document from stdin by supplying the logical filename used for
format routing:

```sh
cat report.pdf | deckprobe -n report.pdf -
```

Process multiple paths or named base64 payloads as JSONL. DeckProbe writes one
compact schema-v2 result for each non-empty input line:

```sh
printf '%s\n' \
  '{"path":"report.pdf"}' \
  '{"path":"deck.pptx"}' | deckprobe --jsonl -t @summary
```

## Use from a coding agent

DeckProbe ships an [Agent Skill](https://agentskills.io) that teaches an agent the
target vocabulary, the report contract, and the exit-status handling, so it stops
guessing flags and stops falling back to unzipping documents by hand.

If you already have the CLI, install the skill for whichever agents this project
uses:

```sh
deckprobe install --skills                     # every agent present in this project
deckprobe install --skills --agent claude -g   # ~/.claude/skills/deckprobe/
deckprobe install --skills --dry-run --pretty  # preview first
```

If you do not, install it straight from this repository:

```sh
npx skills add deckflow/deckprobe
```

Claude Code users can instead take it as a plugin:

```
/plugin marketplace add deckflow/deckprobe
/plugin install deckprobe@deckflow
```

All three deliver the same bytes from [`skills/deckprobe/`](skills/deckprobe/).
Note that `npx skills add` and the plugin install the instructions only — the
skill's first step checks for the CLI and falls back to `npx -y @deckflow/deckprobe`
when it is missing. See
[Installing agent assets](docs/CLI-REFERENCE.md#installing-agent-assets) for the
agent/directory table and the overwrite policy.

## MCP server

An agent with no shell — Claude Desktop, an IDE chat pane, a hosted agent —
reaches DeckProbe through
[deckprobe-mcp-server](https://github.com/deckflow/deckprobe-mcp-server), which
serves this engine over [MCP](https://modelcontextprotocol.io) as four typed
tools: `probe`, `probe_batch`, `list_formats`, and `list_targets`.

```sh
claude mcp add deckprobe -- npx -y @deckflow/deckprobe-mcp
```

```jsonc
// Claude Desktop, Cursor, VS Code, Zed — any client reading mcpServers
{ "deckprobe": { "command": "npx", "args": ["-y", "@deckflow/deckprobe-mcp"] } }
```

It spawns the same native binary this repository ships, returns schema-v2
reports unmodified, and validates tool arguments before the engine runs. It is
listed in the MCP Registry as `io.github.deckflow/deckprobe`.

The skill and the MCP server teach the same vocabulary. Use the skill when the
agent has a shell and you want the CLI's complete surface; use the MCP server
when it does not.

## JavaScript package

The independently published `@deckflow/deckprobe` package ships the `deckprobe`
command for Node and runs the same target-driven Rust engine in WebAssembly for
browsers and Node APIs.

### Install from npm

```sh
npm install @deckflow/deckprobe
```

That installs the `deckprobe` command as well. It is the same native binary the
standalone installers ship, delivered through a per-platform optional
dependency, so every flag, help page, report, and exit code is identical:

```sh
npx @deckflow/deckprobe --help
npx @deckflow/deckprobe -t slide_count deck.pptx
```

Under Node the package also exposes `probeFile()`, which reads a file and
returns the same report the CLI writes for it. Note that it holds the whole
file in memory, while the CLI reads only the paths a probe needs — prefer the
command, or `--jsonl` for batches, on large inputs.

```ts
import { probeFile } from "@deckflow/deckprobe";

const report = await probeFile("deck.pptx", { targets: ["@summary"] });
```

### Browser SDK

In the browser it accepts `File`, `Blob`, `ArrayBuffer`, and `Uint8Array`
inputs; document bytes do not leave the browser.

| Need | Import | Use it when |
|---|---|---|
| Main-thread probe and discovery | `@deckflow/deckprobe` | A small, interaction-adjacent probe can use the UI thread. |
| Off-main-thread probe | `@deckflow/deckprobe/worker` | A user-provided or deep probe must not block rendering. |
| TypeScript types | `@deckflow/deckprobe` | Type `ProbeResult`, `ProbeCallOptions`, or the discovery responses. |

`probe()` initializes WASM lazily. Call `initDeckProbe()` during application
startup or idle time when the first user interaction should use the warm path.

```ts
import { initDeckProbe, probe } from "@deckflow/deckprobe";

await initDeckProbe();

const report = await probe(file, {
  targets: ["@summary", "@security"],
  level: "metadata",
});
```

| Input | Filename handling |
|---|---|
| `File` | Uses `File.name` automatically. |
| `Blob`, `ArrayBuffer`, `Uint8Array` | Pass `name` with a filename extension so DeckProbe can route the format. |

For example, probe fetched bytes with an explicit logical filename:

```ts
const bytes = await fetch("/documents/quarterly-deck").then((response) =>
  response.arrayBuffer(),
);
const report = await probe(bytes, {
  name: "quarterly-deck.pptx",
  targets: ["powerpoint.slide_count"],
});
```

Use the Worker entry point for user-provided files, deep probes, or any flow
where parsing must not block the UI thread. Reuse one worker for a batch and
terminate it when the owning screen or job ends:

```ts
import { createDeckProbeWorker } from "@deckflow/deckprobe/worker";

const worker = createDeckProbeWorker();
try {
  const report = await worker.probe(file, {
    targets: ["@summary"],
    level: "deep",
  });
} finally {
  worker.terminate();
}
```

The Worker entry is a module worker resolved relative to the installed package;
verify that the application's bundler preserves module-worker URLs. Calling
`terminate()` cancels pending probes, so create a new worker for a later batch.
`formats()`, `targets(format)`, `schema()`, and `version()` are available from
the main entry point for discovery and integration tooling. See the
[package guide](packages/deckprobe-js/README.md) for the complete API.

### Bundler setup

The package resolves its WebAssembly binary relative to its own JavaScript
wrapper, so a bundler that relocates the wrapper without the binary breaks
initialization. **Vite's dev server does this in every version** and reports
either `HTTP status code is not ok` or `expected magic word 00 61 73 6d`,
while `vite build` works — exclude the package from dependency
pre-bundling:

```ts
// vite.config.ts
export default defineConfig({
  optimizeDeps: { exclude: ["@deckflow/deckprobe"] },
});
```

Main-thread-only applications can instead pass the binary URL to
`initDeckProbe()` via the `@deckflow/deckprobe/wasm` export. See the package
guide's [bundler notes](packages/deckprobe-js/README.md#bundlers) for both
fixes, the per-version error messages, and the CDN, sub-path, and CSP cases.

## Execution modes

DeckProbe deliberately exposes different execution modes instead of treating
every workload as a new process. Choose the mode that matches the lifetime of
your application:

| Mode | Best for | Lifecycle and trade-off |
|---|---|---|
| Native CLI, single-shot | Shell commands, CI steps, one-off automation | Starts a new process per input; simplest invocation and full end-to-end CLI cost. |
| Native CLI, persistent JSONL | Servers, queues, and high-volume local batches | One `deckprobe --jsonl` process stays alive and processes one JSON record per line; avoids startup cost while keeping every document probe independent. |
| Browser SDK, main thread | Short, interaction-adjacent browser checks | Lowest browser transport overhead, but a deep probe can occupy the UI thread. |
| Browser SDK, module Worker | Upload screens, large files, and deep browser inspection | Keeps the UI responsive; includes byte-copy, message, and response-transfer cost. |

The browser paths run the same planner and bounded engine work as native
execution. In practical terms: use JSONL when a native service processes a
queue; use the main-thread SDK only when the expected probe is small enough to
fit the UI budget; and use the Worker SDK as the default for user files and
deep analysis.

## Common recipes

Probe only low-cost identity targets:

```sh
deckprobe -l h -t @header suspicious.docx
```

Require an exact slide count. The planner selects `presentation.xml` instead of the cheaper saved statistic:

```sh
deckprobe -c x \
  -t slide_count \
  deck.pptx
```

Preview the selected paths without executing non-header probes:

```sh
deckprobe -P -t @default deck.pptx
```

Bound work on an untrusted archive and fail if a requested target cannot be resolved:

```sh
deckprobe -s \
  -b 8388608 \
  -x 16777216 \
  -e 5000 \
  -T 1000 \
  -t format,has_macros \
  upload.docm
```

Discover capabilities from the CLI itself:

```sh
deckprobe --pretty formats
deckprobe --pretty targets --format pdf
deckprobe --pretty targets --format docx
deckprobe --pretty targets --format xlsx
deckprobe --pretty targets --format pptx
deckprobe --pretty targets --format key
deckprobe --pretty targets --format numbers
deckprobe --pretty targets --format pages
```

Target presets are composable:

| Preset | Meaning |
|---|---|
| `@header` | Low-cost identity targets. |
| `@default` | Driver defaults for the selected probe level. |
| `@summary` | Identity, common metadata, and primary structure. |
| `@security` | Encryption, macros, signatures, external relationships, and active content. |
| `@structure` | Format-owned counts, names, dimensions, and structure. |
| `@assets` | Images, media, previews, fonts, and embedded-object summaries. |
| `@quality` | Integrity, repair, extension, and conformance signals. |
| `@format` | Format-owned targets available at the selected level. |
| `@all` | Every target available at the selected level. |

The summary preset excludes statistics that currently require a full-file
path; request those explicitly or through `@structure`.

## Supported formats

| Driver | Profiles | Current inspection depth |
|---|---|---|
| PDF | `.pdf` | Header and Info metadata, page/object counts, xref type, signatures, links, attachments, JavaScript, forms, annotations, and XMP presence. |
| Word | `.docx`, `.docm`, `.dotx`, `.dotm` | OPC metadata, saved statistics, exact paragraph/table structure, security signals, comments, and image assets. |
| Excel | `.xlsx`, `.xlsm`, `.xltx`, `.xltm`, `.xlsb` | OPC metadata, worksheets/names/visibility, shared strings, tables, charts, pivots, security signals, and image assets; XLSB identity only. |
| PowerPoint | `.pptx`, `.pptm`, `.ppsx`, `.ppsm`, `.potx`, `.potm` | OPC metadata, slides/hidden slides, masters/layouts/notes, slide size, security signals, charts, comments, images, and media. |
| Legacy Office | `.doc`, `.dot`, `.xls`, `.xlt`, `.ppt`, `.pps`, `.pot` | Validated CFB main streams, SummaryInformation metadata, macros, embedded-object signals, and core Word/Excel/PowerPoint statistics. |
| Keynote | `.key` | Modern IWA identity/integrity plus slide canvas, orientation, hidden/notes/build/transition slide counts, and referenced table models. |
| Numbers | `.numbers` | Modern IWA identity/integrity, exact ordered sheets, referenced table dimensions, hidden/filtered dimensions, and persisted formula definitions. |
| Pages | `.pages` | Modern IWA identity/integrity, sections, page geometry, change tracking, body-text structural counts, cached pagination, and referenced table models. |

The shared deep IWA path also exposes total archive-object counts, raw numeric
message-type counts, and stable semantic object-class counts. The filename
extension selects the driver path, after which DeckProbe verifies the container
signature and format-specific root objects. Apple iWork support is intentionally
limited to the modern ZIP/IWA generation; legacy XML packages containing
`index.apxl` or `index.xml` return structured `UNSUPPORTED_FORMAT` JSON. A
suffix/content mismatch returns `MALFORMED_INPUT`. Target discovery includes
`applicable` and `supported_levels`, and scenario selectors only include targets
backed by the selected driver's executable paths.

## How probing works

```text
requested targets + confidence + budget
                  │
          extension dispatcher
                  │
       container + type validation
                  │
       lowest-cost valid probe plan
          ┌───────┼────────┬───────────┬────────────┐
         PDF     Word     Excel    PowerPoint    Apple iWork
                   \        |        /             │
                    shared OOXML paths       ZIP/IWA/Snappy/Proto
                  │
       values + evidence + actual cost
```

Each driver owns its targets, format options, candidate paths, and parsing logic. Word, Excel, and PowerPoint share bounded ZIP/OPC/XML paths. Keynote, Numbers, and Pages share a bounded ZIP/plist/Snappy/IWA/Protobuf layer while retaining separate profile validation and targets.

### JSON contract

Schema version 2 uses one JSON envelope on standard output. Top-level `status` is `ok`, `partial`, or `error`; errors include a stable code, message, and exit code. The tracked [JSON Schema](docs/deckprobe-report.schema.json) is suitable for generated clients and Agent tool contracts. Every requested target has an explicit status such as `resolved`, `estimated`, `planned`, `unknown`, `unsupported`, `budget_exceeded`, or `failed`. Resolved evidence includes:

- `value` and its target-defined type;
- `confidence` and numeric `confidence_score`;
- the executed `path` and evidence `source`;
- deterministic physical-byte, expanded-byte, and random-read counters for the whole probe.

`confidence` records how strong the evidence for a value is — `exact` when it was read from the
authoritative structure, `high` when it came from a statistic the authoring application saved,
`medium` when it was inferred from a proxy. The paired `confidence_score` is a **fixed constant per
label** (`0.4`, `0.7`, `0.95`, `1.0`), not a calibrated probability: `0.95` does not mean the value
is right 95% of the time. Likewise `partial` only means some requested target was unresolved — the
other results are still valid, and it is not a verdict on the document's health. See
[Reading confidence and partial results](docs/CLI-REFERENCE.md#reading-confidence-and-partial-results).

Wall-clock `elapsed_ms` is omitted by default so identical inputs and options produce byte-identical JSON. Add `--telemetry` when timing is needed.

Use `--strict` when unresolved targets should make the command exit non-zero.
Use `--view values` when a compact target-to-value map is preferable to the
complete evidence envelope. `deckprobe schema` prints the exact bundled
contract, and `deckprobe completion SHELL` generates completions from the live
command model.

## CLI reference

Start with the built-in help or the complete [CLI reference](docs/CLI-REFERENCE.md):

```sh
deckprobe --help
deckprobe targets --help
```

To generate the man page from the same CLI definition:

```sh
deckprobe generate man > deckprobe.1
man ./deckprobe.1
```

## Current limitations

DeckProbe intentionally does not render files, run OCR, execute macros,
follow external links, or send documents to a remote service.

- PDF metadata currently reads the Info dictionary; XMP merging is not implemented.
- PDF metadata/deep paths use a bounded full in-memory parser after the header path; safe xref recovery does not attempt damaged encrypted PDFs.
- Legacy Office focuses on metadata and core counts rather than complete fidelity for every historical binary record variant.
- `.xlsb` deep workbook parsing is not implemented.
- Legacy XML iWork documents are recognized and rejected; only modern IWA packages are supported.
- Pages exposes persisted page geometry, body-text structure, and cached pagination; rendered text-layout reconstruction is not implemented.
- Network-backed range sources, persistent cache, and plugin loading are not implemented,
  and are **not planned** for now. Both remote reads and a persistent cache would add
  protocol, authentication, invalidation, privacy, and failure-handling surface, and no
  measured workload has yet shown that fetching or rereading remote documents is a
  material cost next to the local-file and byte-input paths. A proposal would need a
  representative corpus with sizes, repeat-probe frequency, bytes transferred, and
  latency before the work is scheduled.

## Development

```sh
cargo build -p deckprobe
cargo test --workspace
cargo check -p deckprobe-wasm --target wasm32-unknown-unknown
(cd packages/deckprobe-js && npm ci && npm run build && npx playwright install chromium && npm test)
```

Every release is validated by the maintainers' quality gate—format and lint
checks, workspace tests, the Browser SDK contract tests, and a
correctness-and-performance comparison against the previous release—before it
is published to this repository.

Contributions are welcome—read [CONTRIBUTING.md](CONTRIBUTING.md). Treat every input document as untrusted and report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## License

DeckProbe is available under the [MIT License](LICENSE).
