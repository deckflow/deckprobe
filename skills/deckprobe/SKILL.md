---
name: deckprobe
description: >-
  Inspect PDF, Microsoft Office (docx/xlsx/pptx and legacy doc/xls/ppt), and Apple iWork
  (key/numbers/pages) files without opening or rendering them. Use for page and slide and sheet
  counts, title/author/created metadata, encryption and macro and digital-signature and JavaScript
  risk signals, sheet and table structure, embedded assets and fonts, corruption checks, and
  verifying a file really is the format its extension claims. Also use to inventory many documents
  at once. Returns bounded, deterministic JSON on stdout. Never renders, never runs macros, never
  follows external references, never sends the file anywhere.
license: MIT
compatibility: >-
  Needs the deckprobe CLI on PATH, or Node.js with network access to run it through npx. Needs a
  shell tool and local read access to the file. Probing itself needs no network.
allowed-tools: Bash(deckprobe:*)
metadata:
  tool: deckprobe
  homepage: https://github.com/deckflow/deckprobe
  deckprobe-skill-format: "1"
---

# DeckProbe

`ffprobe` for documents. One shell command answers a specific question about a PDF, Office, or iWork
file; the answer comes back as JSON on stdout and the exit status carries the verdict.

Reach for it when a task needs facts **about** a document — how many slides, is it encrypted, does it
have macros, is it corrupt, is this really a `.pptx` — rather than the text inside it. It is safe on
untrusted input: nothing is rendered, no macro runs, no external reference is followed.

## 1. Make sure it runs

`npx skills add` installs this document only, not the binary. Check first:

```bash
deckprobe --version
```

- Prints a version → use `deckprobe` as written throughout this document.
- Command not found, but `node` exists → **the command is `npx -y @deckflow/deckprobe`**. Prefix every
  example below with it. First run downloads the package; later runs are cached.
- Neither → ask the user to install one of:
  - `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/deckflow/deckprobe/releases/latest/download/deckprobe-installer.sh | sh`
  - `npm install -g @deckflow/deckprobe`
  - `cargo install --git https://github.com/deckflow/deckprobe --locked deckprobe`

If you cannot get it running, say so and stop. Do **not** fall back to unzipping the package, grepping
XML, or parsing PDF bytes by hand — avoiding exactly that is the point of this tool.

## 2. The shape of every call

```bash
deckprobe [-t TARGETS] [-l LEVEL] [-c CONFIDENCE] [--view values] [--pretty] FILE
```

Start here when you do not yet know what you need:

```bash
deckprobe -t @summary --view values --pretty report.pdf
```

`--view values` returns a compact `target -> value` map. Drop it when you also need each value's
confidence, the path that produced it, and the measured cost.

**Check the exit status before parsing stdout.** A non-zero status still writes valid JSON, but it is
an error envelope, not a report.

## 3. Choosing targets

`-t` accepts short names (`slide_count`), canonical names (`powerpoint.slide_count`), and selector
presets. It is repeatable and comma-separated: `-t @header,title,page_count`.

| Selector | Expands to |
| --- | --- |
| `@header` | Container identity only — format, size, extension match, encryption flag |
| `@default` | The driver's defaults for the active level. Used when `-t` is omitted |
| `@summary` | Identity, common metadata, and primary structure |
| `@security` | Encryption, macros, signatures, external references, active content |
| `@structure` | Format-owned counts, names, and dimensions |
| `@assets` | Images, media, previews, fonts, embedded objects |
| `@quality` | Integrity, repair, extension match, conformance |
| `@format` | Every format-specific target at the active level |
| `@all` | Everything available at the active level |

`@summary` deliberately omits a statistic the current driver can only get from a full-file read. PDF
page count is the notable case — ask for `page_count` or `@structure` explicitly.

**Never guess a target name.** The tool documents itself:

```bash
deckprobe targets --format pptx --pretty   # ids, aliases, min level, value type, cost class
deckprobe formats --pretty                 # drivers, profiles, support boundaries
deckprobe schema --pretty                  # the authoritative report JSON Schema
```

`references/targets.md` has the full catalogue per format if you would rather read than run.

## 4. Probe level and confidence

`-l` / `--level` picks the budget and which paths are eligible. Accepts `h`/`m`/`d` too.

| Level | Use it for |
| --- | --- |
| `header` | Identity and container properties. ~500 ms budget |
| `metadata` | **Default.** Bounded metadata and common structural counts. ~500 ms |
| `deep` | Higher-cost paths. ~5 s — only when a target's `min_level` says `deep` |

`-c` / `--confidence` filters eligible paths: `low` < `medium` < `high` (default) < `exact`. Lower it
when a target comes back unresolved and an approximate answer is acceptable; raise it with
`-C target=exact` for a single target that must be authoritative.

## 5. Reading the report

```jsonc
{
  "schema_version": 2,
  "status": "ok",              // "ok" | "partial" | "error"
  "input":  { "display_name": "deck.pptx", "source_kind": "local_file" },
  "driver": { "id": "powerpoint", "profile": "pptx" },
  "results": {
    "powerpoint.slide_count": {
      "status": "resolved",     // see the eight values below
      "value": 31,
      "confidence": "high",     // none | low | medium | high | exact
      "path": "powerpoint.app_statistics",
      "source": "docProps/app.xml saved statistic"
    }
  },
  "execution": { "unresolved_targets": [], "actual_cost": { } },
  "diagnostics": []
}
```

A result's `status` is one of `resolved`, `estimated`, `planned`, `unknown`, `unsupported`, `invalid`,
`budget_exceeded`, `failed`. Only `resolved` and `estimated` carry a `value`. `unknown` is the common
one: the path ran and the document simply does not record that fact.

- The report's own `status` is `ok` or `partial`. `partial` is **not** a failure — at least one
  requested target could not be resolved at the requested confidence, and the rest are still valid.
  Check `execution.unresolved_targets`.
- Add `--strict` when an unresolved target must fail the command. The full report is still written,
  and the exit status becomes `5`.
- `results` is keyed by the canonical target name even when you asked with a short alias.

## 6. Exit status — and what to do about it

| Status | Meaning | Your next move |
| ---: | --- | --- |
| `0` | Success | Parse stdout. Unresolved targets are allowed unless `--strict` |
| `1` | Invalid request or unsupported target | You named a target this format has no path for. Run `deckprobe targets --format <fmt>` |
| `2` | CLI syntax error, or the file is missing/unreadable | Fix the flags or the path |
| `3` | Unsupported or unrecognized format | Check the extension against `deckprobe formats` |
| `4` | Malformed input or budget exceeded | Genuinely damaged file, or raise `-b`/`-x`/`-e`/`-T`. See `references/limits.md` |
| `5` | `--strict` and a target was unresolved | The report is on stdout; decide whether the missing target matters |
| `6` | Internal failure | A bug. Report it |

## 7. Many files at once

```bash
printf '%s\n' '{"path":"a.pdf"}' '{"path":"b.pptx"}' | deckprobe --jsonl -t @summary
```

One compact JSON per non-empty input line. A per-record error does not stop the run; the process
exits with the highest per-record status. Each record is a JSON string path, `{"path":"..."}`, or
`{"name":"report.pdf","data_base64":"..."}`.

For bytes on stdin, `-n` supplies the logical filename that selects the format:

```bash
curl -sL "$url" | deckprobe -n download.pdf -t @security -
```

## 8. What DeckProbe will not do

It has no renderer, no OCR, no text extraction, and no macro interpreter. It never opens a network
connection and never resolves an external reference. Legacy XML iWork packages are rejected by
design — only modern IWA is supported. `.xlsb` is identity-only. PDF XMP is not merged into the
metadata targets.

Routing is by filename extension, then verified against the container. Renaming a `.pptx` to `.docx`
returns `MALFORMED_INPUT`, and `-f`/`--input-format` is an assertion, not an override — it cannot
force an unrelated parser onto a file.

Never parse stderr. Every result, including every error, is one JSON value on stdout.

## References

Read these only when you need them:

- **`references/targets.md`** — the full target catalogue per format, with aliases and minimum
  levels. Read before naming a target you have not verified.
- **`references/recipes.md`** — task → command lookup for the common questions.
- **`references/output.md`** — the complete report shape, error codes, and cost accounting, for when
  you are consuming the JSON programmatically.
- **`references/limits.md`** — budgets, the `-b`/`-x`/`-e`/`-T` overrides, `-O` format options, and
  how to diagnose an exit `4`.
