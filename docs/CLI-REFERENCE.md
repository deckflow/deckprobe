# DeckProbe CLI reference

DeckProbe routes local files, raw stdin bytes, and JSONL records through the
same source-independent engine. A logical filename extension selects the PDF,
Microsoft Office, or modern Apple iWork path; DeckProbe then verifies the
container and internal type, executes only the paths needed for the requested
targets, and emits structured JSON.

## Synopsis

```text
deckprobe [OPTIONS] <INPUT>
deckprobe --jsonl [OPTIONS]
deckprobe <COMMAND> [OPTIONS]
```

Run `deckprobe -h` for a compact option list or `deckprobe --help` for selector details, examples, and exit statuses. Every discovery command also has its own long help:

```bash
deckprobe formats --help
deckprobe targets --help
deckprobe generate --help
deckprobe schema --help
deckprobe completion --help
deckprobe install --help
```

## Quick examples

Inspect a document with the default metadata-level probe:

```bash
deckprobe report.pdf
```

Request a specific target and pretty-print the report:

```bash
deckprobe -t slide_count --pretty deck.pptx
```

Request several targets in one pass:

```bash
deckprobe \
  -t format,title,page_count \
  report.pdf
```

The target option is repeatable. Use short target names after the input format
has been detected:

```bash
deckprobe -t title -t page_count report.pdf
deckprobe -t title,page_count report.pdf
```

Inspect every target available to the deep profile:

```bash
deckprobe -l d -t @all workbook.xlsx
```

Preview the selected execution paths without executing driver probe paths:

```bash
deckprobe -P -p report.pdf
```

Treat unresolved targets as a failed command while retaining the JSON report:

```bash
deckprobe -s -t format,page_count report.pdf
```

Return a compact target-to-value envelope when evidence details are not needed:

```bash
deckprobe -t @summary --view values report.pdf
```

Probe bytes from stdin using a logical filename with an extension:

```bash
cat report.pdf | deckprobe -n report.pdf -
```

Probe several paths from JSONL:

```bash
printf '%s\n' '{"path":"report.pdf"}' '{"path":"deck.pptx"}' \
  | deckprobe --jsonl -t @summary
```

## Output contract

Single-input probes, discovery, and failed commands write exactly one JSON
value to standard output. JSONL mode writes one compact JSON value per non-empty
input line. Probe reports use schema version `2` and contain these top-level
fields:

```text
schema_version  tool_version  status  input  driver  results  execution  diagnostics
```

- `results` is keyed by target name. Each result includes its status, confidence, selected path, and source; resolved results also contain `value`.
- `status` is `ok` when every requested target satisfies the request, or `partial` when one or more results are unresolved or below the requested confidence.
- `execution` records the probe level, selected paths, estimated and actual cost,
  unresolved targets, and any zero-additional-path `piggyback_targets`.
- `diagnostics` contains structured warnings produced while planning or probing.
- `--pretty` changes whitespace only; it works before or after the `formats` and `targets` subcommands.
- Failures use `status: "error"` and an `error` object containing stable `code`, `message`, and `exit_code` fields.
- Default reports are deterministic: `actual_cost` includes byte and seek counters but omits wall-clock time. `--telemetry` opts into `elapsed_ms`.
- With `--strict`, DeckProbe still writes the complete report to standard output, then exits with status `5` if any requested target is unresolved.
- `--view values` returns `schema_version`, input/driver identity, a compact
  `values` map, unresolved targets, and diagnostics. The default `--view full`
  keeps the complete evidence report.

For scripts, check the exit status before consuming standard output unless status `5` is an expected result.

## Input modes

### Local path

The default positional input is a regular local file. The report uses
`source_kind: "local_file"` and preserves the basename as `display_name`.

### Raw stdin

Use positional `-` and provide `-n`/`--stdin-name NAME`. The name must contain the
extension used for routing; `--input-format` remains only an assertion and does
not replace it. Stdin is buffered at the CLI boundary and rejected as soon as it
exceeds the active physical-byte budget.

```bash
cat workbook.xlsx | deckprobe -n workbook.xlsx -
```

The report uses `source_kind: "stdin"`.

### JSONL multi-input

`--jsonl` reads stdin one line at a time. Each non-empty line may be:

```json
"/absolute/or/relative/report.pdf"
{"path":"report.pdf"}
{"name":"upload.pdf","data_base64":"JVBERi0xLjcK"}
```

`name` may override the display/routing name of a path record. A byte record
requires both `name` and `data_base64`; `base64` is accepted as an alias.
Global target, confidence, format-option, budget, strict, plan, and view options
apply to every record. Pretty output conflicts with JSONL because each output
must remain on one line. Record errors are emitted in place and later records
continue; the process exits with the highest record exit status.

## Selecting targets

Pass one or more short target names to repeatable `-t`/`--targets`; values may
also be comma-separated. The input filename extension supplies the format, so
the examples do not repeat a format prefix. The default selector is `@default`.
Reports and discovery output retain stable canonical target keys for machine
consumers; those keys are omitted from the interactive examples below.

| Selector | Expands to |
| --- | --- |
| `@header` | Header-level identity targets |
| `@default` | The driver's defaults for the active `--probe-level` |
| `@summary` | Identity, common metadata, and primary format structure |
| `@security` | Encryption, macro, signature, external, and active-content signals |
| `@structure` | Format-owned counts, names, dimensions, and structure |
| `@assets` | Asset, preview, image, media, font, and embedded-object targets |
| `@quality` | Integrity, repair, extension, and conformance targets |
| `@format` | Format-specific targets available at the active level |
| `@all` | Every target available at the active level |

`@summary` deliberately omits a format statistic when the current driver can
only obtain it through a full-file path. For example, request PDF page count
through `@structure` or `page_count` until a range-aware page-tree path is
available.

Target names and presets may be mixed:

```bash
deckprobe -t @header,title,page_count report.pdf
deckprobe -t @summary -t @security report.pdf
```

Optional targets are returned only when a path already selected for a required
target produces them at the requested confidence. They never add another path
and do not make the report partial when absent:

```bash
deckprobe -t page_count -o object_count report.pdf
deckprobe -t page_count -o object_count -N report.pdf
```

Discover the exact targets, minimum levels, value types, and format options for a profile with `targets`:

```bash
deckprobe targets --format pdf --pretty
deckprobe targets pdf --pretty
deckprobe targets --format docx --pretty
deckprobe targets --format xlsx --pretty
deckprobe targets --format pptx --pretty
deckprobe targets --format key --pretty
deckprobe targets --format numbers --pretty
deckprobe targets --format pages --pretty
```

Accepted format names include `pdf`, `word`/`docx`/`docm`, `excel`/`xlsx`/`xlsm`, `powerpoint`/`pptx`/`pptm`, `keynote`/`key`, `numbers`, `pages`, and `legacy`/`doc`/`xls`/`ppt`.

## Probe level and confidence

`--probe-level`/`--level` (`-l`) controls both the resource budget profile and
which probe paths are eligible:

| Level | Intended use |
| --- | --- |
| `header` | Container identity and header-level properties |
| `metadata` | Bounded document metadata and common structural counts; this is the default |
| `deep` | Higher-cost paths needed by deep targets |

Levels also accept `h/m/d`, `l0/l1/l2`, and `0/1/2`.

`--minimum-confidence`/`--confidence` (`-c`) filters eligible paths. Its values,
in increasing order, are `low`, `medium`, `high`, and `exact`; `l/m/h/x` are
accepted shorthands and the default is `high`.

Use repeatable `-C`/`--target-confidence TARGET=LEVEL` for individual overrides. A
short target alias is accepted when it is unambiguous:

```bash
deckprobe -t slide_count,orientation \
  -C slide_count=x \
  deck.pptx
```

An explicitly named target can be valid for the detected format but unavailable at the chosen level or confidence. If some other requested target can be planned, the report records the unavailable target under `execution.unresolved_targets`. If no requested target has an eligible path, DeckProbe exits with status `1` as an unsupported-target request.

## Reading confidence and partial results

### What the confidence labels mean

`confidence` describes how strong the evidence for one value is, as judged by the path that
produced it. It is not a measured accuracy rate.

| Label | Score | What normally backs it |
| --- | --- | --- |
| `exact` | `1.0` | Read directly from the authoritative structure in the container |
| `high` | `0.95` | A statistic the authoring application saved, such as the slide count in `docProps/app.xml`. Authoritative unless that application left it stale |
| `medium` | `0.7` | Inferred from a proxy, such as counting `xl/worksheets/sheet*.xml` parts instead of reading the workbook's declared sheets |
| `low` | `0.4` | Weak or indirect evidence |
| `none` | `0.0` | Accompanies a result that carries no value |

**`confidence_score` is a fixed constant per label, not a calibrated probability.** `0.95` does not
mean the value is correct 95% of the time on real-world files; no corpus measurement backs these
numbers. Use them to order or threshold results, never to report an accuracy figure to a user.

### Report status versus target status

The report's own `status` is `ok` or `partial`. A result's `status` is one of eight values, and the
two answer different questions.

`partial` means at least one requested target could not be resolved at the requested confidence. It
says nothing about whether the document is damaged or unsafe — the remaining results are still
valid.

```bash
deckprobe -t slide_count,author --pretty deck.pptx
```

```jsonc
{
  "status": "partial",                       // because author could not be resolved
  "results": {
    "powerpoint.slide_count": {
      "status": "resolved", "value": 31,
      "confidence": "high", "path": "powerpoint.app_statistics",
      "source": "docProps/app.xml saved statistic"
    },
    "document.author": {
      "status": "unknown",                   // the path ran; the file records no author
      "confidence": "none"
    }
  },
  "execution": { "unresolved_targets": ["document.author"] }
}
```

The slide count here is perfectly good. Treating `partial` as a failure would discard it.

Contrast that with a structural target the format cannot answer at all:

```bash
deckprobe -l d -t corrupted report.pdf     # exits 1
```

`corrupted` and `missing_assets` are declared for every modern format, but only the iWork drivers
implement a path for them. Naming one on a PDF or OOXML file is an unsupported-target request, so
DeckProbe exits `1` rather than returning a report. Use the `@quality` selector to get whatever the
active driver actually supports.

| Result `status` | Carries `value` | Meaning |
| --- | --- | --- |
| `resolved` | yes | Obtained at or above the requested confidence |
| `estimated` | yes | Obtained, but an estimate |
| `unknown` | no | The path ran; the document does not record this fact. A normal answer, not an error |
| `unsupported` | no | This format has no path for the target |
| `invalid` | no | The document records something that fails validation |
| `budget_exceeded` | no | A limit stopped this target specifically |
| `failed` | no | The path errored |
| `planned` | no | `--plan-only` only |

Distinguish `"value": null` on a `resolved` result — the field exists and is empty, which is an
answer — from `status: "unknown"`, where the probe could not answer.

Use `--strict` when an unresolved target must fail the command; it exits `5` and still writes the
full report.

## Input interpretation and format options

DeckProbe uses the normalized filename extension to select a format path, then verifies its signature and internal type. Renaming a PPTX to DOCX, for example, returns `MALFORMED_INPUT`. `-f`/`--input-format` adds another assertion; it does not force an unrelated parser onto the file:

```bash
deckprobe --input-format pdf report.pdf
deckprobe --input-format powerpoint deck.pptx
deckprobe --input-format iwork deck.key
```

For `.key`, `.numbers`, and `.pages`, validation requires `Index/Document.iwa`, `Metadata/Properties.plist`, and the expected IWA root-object family. At `deep`, the bounded Snappy/Protobuf path scans every IWA entry and exposes archive/message-type inventories plus stable Keynote slide state, Numbers table models, and Pages page/text structure. Legacy XML iWork packages remain outside the support boundary and return `UNSUPPORTED_FORMAT` with a message that modern IWA is required.

Pass driver settings with repeatable namespaced `KEY=VALUE` arguments. Query `deckprobe targets --format FORMAT` for the live option list, allowed values, and defaults:

```bash
deckprobe \
  -O repair_xref=safe \
  -O max_objects=50000 \
  report.pdf
```

`-O`/`--option` is an alias for `--format-option`. After driver detection, an
unambiguous local key can omit its namespace:

```bash
deckprobe -O repair_xref=safe -O max_objects=50000 report.pdf
```

When the same option is repeated, the last value wins. Unknown options and values are rejected by the selected driver.

## Resource limits

The level profile supplies defaults, and these options override individual limits:

| Option | Limit |
| --- | --- |
| `-b` / `--probe-size BYTES` | Physical bytes read by probe paths; `--probesize` is an alias |
| `-x` / `--max-expanded-bytes BYTES` | Cumulative decompressed bytes |
| `-e` / `--max-archive-entries COUNT` | ZIP/OPC entry count |
| `-T` / `--timeout-ms MILLISECONDS` | Wall-clock probe budget |

A budget violation exits with status `4`. The successfully consumed cost is reported under `execution.actual_cost` when a report can be produced.

The built-in defaults are designed for a fast CLI response: 500 ms at `header`, 500 ms at `metadata`, and 5 seconds only for an explicitly selected `deep` probe. Header allows up to 4 MiB of physical and expanded data and 4,096 archive entries so realistic iWork central directories and `Document.iwa` can be validated. These are cooperative hard bounds on DeckProbe I/O and parsing checkpoints; callers that need a process-level deadline should still enforce one around the CLI.

## Discovery commands

List the supported drivers, profiles, and support boundaries:

```bash
deckprobe formats --pretty
```

List the target and format-option schemas for one profile:

```bash
deckprobe targets --format pdf --pretty
deckprobe targets pdf --pretty
```

Discovery includes each target's compatibility `value_type`, JSON Schema
fragment, aliases, selector membership, minimum level, cost class, and complete
selector expansions for `header`, `metadata`, and `deep`.

Print the exact report schema bundled into the running binary or generate shell
completion source:

```bash
deckprobe schema --pretty
deckprobe completion bash > deckprobe.bash
deckprobe completion zsh > _deckprobe
```

`--pretty` is a global output flag, so both placements are valid:

```bash
deckprobe --pretty formats
deckprobe formats --pretty
```

## Installing agent assets

`deckprobe install` writes support files into a local directory. Where `generate man` and
`completion` emit to standard output, `install` resolves a conventional destination and applies one
policy to everything it writes: an idempotent created/updated/unchanged comparison, a `--force` gate,
`--dry-run`, and a single JSON receipt.

```bash
deckprobe install                                     # the default artifact set: the agent skill
deckprobe install --skills --dry-run --pretty         # preview, writing nothing
deckprobe install --skills --agent claude --global    # ~/.claude/skills/deckprobe/
deckprobe install --skills --dir ./.claude/skills     # explicit skills container
deckprobe install --man --global
deckprobe install --completions zsh --dir ~/.zfunc
```

Selectors combine, and each resolves its own destination, so `--skills --man` installs both in one
run. With no selector at all, the default set is installed, which today is `--skills`.

| Selector | Default destination | With `--dir D` |
| --- | --- | --- |
| `--skills` | the skills directory for each resolved `--agent` | `D` is the skills container; the skill lands in `D/deckprobe/` |
| `--man` | `./man`, or `$XDG_DATA_HOME/man/man1` (else `~/.local/share/man/man1`) with `--global` | files land directly in `D` |
| `--completions SHELL` | none — `--dir` is required, because user completion directories are not standardized | files land directly in `D` |

### Agents

`--agent` is repeatable and comma-separated. `--dir` bypasses it entirely and conflicts with both
`--agent` and `--global`.

| Agent | Project | User (`--global`) |
| --- | --- | --- |
| `claude` (alias `claude-code`) | `.claude/skills` | `~/.claude/skills` |
| `codex` (alias `codex-cli`) | `.agents/skills` | `~/.codex/skills` |
| `cursor` | `.agents/skills` | `~/.cursor/skills` |
| `opencode` | `.agents/skills` | `~/.config/opencode/skills` |
| `gemini` (alias `gemini-cli`) | `.agents/skills` | `~/.gemini/skills` |
| `copilot` (alias `github-copilot`) | `.agents/skills` | `~/.copilot/skills` |
| `windsurf` | `.windsurf/skills` | `~/.codeium/windsurf/skills` |
| `cline`, `zed`, `agents` (alias `universal`) | `.agents/skills` | `~/.agents/skills` |

`auto` is the default: it selects every agent whose directory already exists at the chosen scope and
falls back to the vendor-neutral `agents` layout when none does. `all` selects every row. Several
agents share `.agents/skills`, so destinations are deduplicated and the receipt lists every agent
served by each one.

DeckProbe deliberately does not track the full ecosystem of agent directories. For anything outside
this table, use `--dir`, or install through the skills CLI, which maintains a much larger table:

```bash
npx skills add deckflow/deckprobe -a <agent>
```

### Receipt and overwrite policy

```json
{
  "schema_version": 2,
  "tool_version": "2.3.1",
  "status": "ok",
  "install": {
    "artifacts": ["skills"],
    "scope": "project",
    "dry_run": false,
    "force": false,
    "targets": [
      {
        "artifact": "skills",
        "name": "deckprobe",
        "agents": ["claude"],
        "directory": "./.claude/skills/deckprobe",
        "files": [{ "path": "SKILL.md", "bytes": 9069, "action": "created" }],
        "orphaned": []
      }
    ]
  }
}
```

- A file whose contents already match is reported `unchanged` and is not rewritten, so re-running
  `install` is a safe no-op.
- A skill directory DeckProbe previously wrote is recognized by a marker in its `SKILL.md` and is
  refreshed in place, so upgrading needs no `--force`.
- A `SKILL.md` without that marker belongs to somebody else. The command fails with exit `1` and
  writes nothing at all — including for other agents in the same run — until `--force` is given.
- `orphaned` lists files in the destination that this version no longer ships. They are reported,
  never deleted.
- `--dry-run` performs the same validation and produces the same receipt without writing.

Every destination is resolved and validated before anything is written, so a rejected run leaves no
half-installed tree behind -- across artifacts as well as across agents.

Exit statuses follow the table below: `1` for a refused overwrite, a missing `--dir` for
`--completions`, or an unresolvable home directory; `2` for a contradictory flag combination or a
write failure.

The skill this installs is the same content published at `github.com/deckflow/deckprobe`, so
`npx skills add deckflow/deckprobe` and `/plugin marketplace add deckflow/deckprobe` deliver
identical bytes.

## Exit status

| Status | Meaning |
| ---: | --- |
| `0` | Success. Unresolved targets are allowed unless `--strict` is used. |
| `1` | Invalid request or unsupported target. This includes a missing input argument. |
| `2` | Command-line syntax error or source I/O error. |
| `3` | Unsupported or unrecognized input format. |
| `4` | Malformed input or probe budget exceeded. |
| `5` | At least one requested target is unresolved and `--strict` was used. A JSON report is still written. |
| `6` | Internal parser or report-serialization failure. |

Syntax and runtime failures use the same stable, script-friendly JSON shape on standard output:

```json
{
  "schema_version": 2,
  "tool_version": "2.2.0",
  "status": "error",
  "error": {
    "code": "MALFORMED_INPUT",
    "message": "malformed input: .docx package is missing required main part word/document.xml",
    "exit_code": 4
  }
}
```

Human help and version output remain normal text for `--help` and `--version`.

## Man pages

Generate the main roff manual page from the same command model as `--help`:

```bash
deckprobe generate man > deckprobe.1
man ./deckprobe.1
```

For packaging or a local `MANPATH`, generate the main page and one page for each subcommand:

```bash
deckprobe generate man -d ./man
man -M ./man deckprobe
man -M ./man deckprobe-targets
```

The generator creates its output directory when needed and writes
`deckprobe.1`, `deckprobe-formats.1`, `deckprobe-targets.1`,
`deckprobe-generate.1`, `deckprobe-schema.1`, `deckprobe-completion.1`, and
`deckprobe-install.1`.

`deckprobe install --man` writes the same set into a conventional location and reports the result as
JSON. See [Installing agent assets](#installing-agent-assets).
