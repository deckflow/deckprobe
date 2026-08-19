# The JSON contract

Every invocation writes exactly one JSON value to stdout — success, partial, or error. JSONL mode
writes one compact JSON value per non-empty input line. Nothing useful goes to stderr.

The authoritative schema ships inside the binary:

```bash
deckprobe schema --pretty
```

## Probe report (`--view full`, the default)

Top-level fields:

```text
schema_version  tool_version  status  input  driver  results  execution  diagnostics
```

| Field | Notes |
| --- | --- |
| `schema_version` | `2` |
| `tool_version` | The binary's version |
| `status` | `ok` or `partial` — never `error`; failures use the envelope below |
| `input` | `display_name`, `source_kind` (`local_file` \| `stdin` \| `jsonl_bytes`), `file_size` |
| `driver` | `id` (e.g. `powerpoint`) and `profile` (e.g. `pptx`) |
| `results` | Keyed by **canonical** target name, even when requested by alias |
| `execution` | Level, selected paths, estimated and actual cost, unresolved targets |
| `diagnostics` | Structured warnings from planning and probing |

### A result entry

```json
{
  "target": "powerpoint.slide_count",
  "status": "resolved",
  "value": 31,
  "confidence": "high",
  "confidence_score": 0.95,
  "path": "powerpoint.app_statistics",
  "source": "docProps/app.xml saved statistic"
}
```

`status` is one of:

| Status | Meaning |
| --- | --- |
| `resolved` | A value was obtained at or above the requested confidence |
| `estimated` | A value was obtained, but it is an estimate |
| `planned` | `--plan-only`: this path would have run |
| `unknown` | The path ran; the document does not record this fact |
| `unsupported` | This format has no path for this target |
| `invalid` | The document records something that does not validate |
| `budget_exceeded` | A limit stopped this target specifically |
| `failed` | The path errored |

Only `resolved` and `estimated` carry `value`. `confidence` is `none` \| `low` \| `medium` \| `high` \|
`exact`, with `confidence_score` as the numeric form.

Distinguish carefully:

- `"value": null` on a `resolved` result — the field exists and is empty. That is an answer.
- `status: "unknown"` — the probe could not answer. Not the same thing.

### `execution`

```json
{
  "probe_level": "metadata",
  "paths": ["powerpoint.app_statistics", "ooxml.core_properties"],
  "estimated_cost": 12,
  "actual_cost": {
    "physical_bytes_read": 16730,
    "expanded_bytes": 15181,
    "random_reads": 10
  },
  "unresolved_targets": ["document.author"]
}
```

`actual_cost` is deterministic by design — byte and seek counters only, no wall-clock. `--telemetry`
opts into an `elapsed_ms` field, which makes output non-reproducible; leave it off when diffing
reports.

`piggyback_targets` lists optional (`-o`) targets that came back for free.

## Values report (`--view values`)

Compact envelope for when evidence does not matter:

```json
{
  "schema_version": 2,
  "tool_version": "2.3.1",
  "status": "ok",
  "view": "values",
  "input":  { "display_name": "deck.pptx", "source_kind": "local_file", "file_size": 306716 },
  "driver": { "id": "powerpoint", "profile": "pptx" },
  "values": { "powerpoint.slide_count": 31 },
  "unresolved_targets": [],
  "piggyback_targets": [],
  "diagnostics": []
}
```

Note the shape difference: `unresolved_targets` is **top-level** here, not under `execution`.

## Error envelope

```json
{
  "schema_version": 2,
  "tool_version": "2.3.1",
  "status": "error",
  "error": {
    "code": "SOURCE_IO",
    "message": "source I/O error: No such file or directory (os error 2)",
    "exit_code": 2
  }
}
```

`code` is stable and one of:

| Code | Exit |
| --- | ---: |
| `CLI_SYNTAX` | 2 |
| `INVALID_REQUEST` | 1 |
| `UNSUPPORTED_TARGET` | 1 |
| `SOURCE_IO` | 2 |
| `UNSUPPORTED_FORMAT` | 3 |
| `MALFORMED_INPUT` | 4 |
| `BUDGET_EXCEEDED` | 4 |
| `PARSER_FAILURE` | 6 |

Switch on `error.code`, not on the message text.

`--strict` is the exception to "non-zero means envelope": it exits `5` while still writing the
complete probe report, because the report is the useful part.

## Determinism

Two runs over the same bytes produce byte-identical output as long as `--telemetry` is off. That
makes reports safe to store as fixtures, diff in CI, and cache.
