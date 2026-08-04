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
`deckprobe-generate.1`, `deckprobe-schema.1`, and
`deckprobe-completion.1`.
