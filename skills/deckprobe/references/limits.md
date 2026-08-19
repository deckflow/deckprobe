# Budgets, limits, and format options

DeckProbe bounds its own work. The level profile supplies defaults; four flags override them.

## What the defaults are

| Level | Wall clock | Notes |
| --- | --- | --- |
| `header` | 500 ms | Up to 4 MiB physical and expanded, 4,096 archive entries — enough to validate a realistic iWork central directory and `Document.iwa` |
| `metadata` | 500 ms | The default level |
| `deep` | 5 s | Only for an explicitly selected deep probe |

These are tuned for a fast CLI response, not for the largest file you own.

## Overriding them

| Flag | Limit |
| --- | --- |
| `-b` / `--probe-size BYTES` | Physical bytes read by probe paths (alias `--probesize`) |
| `-x` / `--max-expanded-bytes BYTES` | Cumulative decompressed bytes |
| `-e` / `--max-archive-entries COUNT` | ZIP/OPC entry count |
| `-T` / `--timeout-ms MILLISECONDS` | Wall-clock probe budget |

```bash
deckprobe -l d -T 30000 -x 500000000 -t @all huge.pptx
```

## Diagnosing exit 4

Exit `4` covers both `MALFORMED_INPUT` and `BUDGET_EXCEEDED` — read `error.code` to tell them apart.

- `BUDGET_EXCEEDED` → the file is fine, the budget was too small. Raise the specific limit named in
  the message. `execution.actual_cost` in a successful run on a similar file tells you what to ask
  for.
- `MALFORMED_INPUT` → the container or its required internal part did not validate. Common causes:
  the extension does not match the content, the ZIP central directory is damaged, or a required OOXML
  or IWA part is missing. This is a real answer about the file, not a tuning problem.

Estimate before you pay:

```bash
deckprobe -P -p -l d -t @all huge.pdf
```

`--plan-only` reports the paths that would run and their `estimated_cost` without executing driver
probe paths.

## These are cooperative bounds

They limit DeckProbe's own I/O and parsing at checkpoints. They are not a process-level deadline and
not a sandbox. A caller that needs a hard kill should still impose one around the CLI — `timeout 10s
deckprobe ...` — and run untrusted input with the filesystem permissions it deserves.

What the bounds *do* guarantee: no rendering, no macro execution, no network access, no external
reference resolution, and no unbounded decompression.

## Format options

`-O` / `--format-option` takes namespaced `KEY=VALUE`, repeatable. After the driver is detected an
unambiguous local key may drop its namespace. Last value wins; unknown keys and values are rejected.

| Option | Default | Values |
| --- | --- | --- |
| `pdf.repair_xref` | `safe` | `safe`, `none` — bounded xref recovery for damaged-but-readable PDFs |
| `pdf.max_objects` | `100000` | u64 |
| `excel.workbook_path` | `auto` | enum |

```bash
deckprobe -O repair_xref=none -t page_count report.pdf
```

Set `repair_xref=none` when you want to know whether a PDF is clean *without* recovery. With the
default `safe`, check the `repaired` target instead — `true` means recovery was needed.

The live list, with allowed values and descriptions, is always:

```bash
deckprobe targets --format pdf --pretty   # -> "format_options"
```

## Support boundaries

`deckprobe formats --pretty` reports each driver's boundary. The current ones worth knowing:

- `.xlsb` — identity only; structural targets stay unresolved.
- iWork — modern IWA only. Legacy XML packages return `UNSUPPORTED_FORMAT`.
- Legacy `.doc`/`.xls`/`.ppt` — metadata and format statistics only; check `content_probe_supported`.
- PDF XMP is not merged into the `document.*` metadata targets; `has_xmp` reports its presence.
