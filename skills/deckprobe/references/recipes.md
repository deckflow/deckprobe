# Recipes

Task → command. Every one of these writes JSON to stdout; check the exit status first.

## Identity and triage

**What is this file, cheaply?**
```bash
deckprobe -t @header --view values --pretty suspicious.bin
```
Header level only — no metadata paths run. Good first move on anything untrusted.

**Is the extension lying?**
```bash
deckprobe -t format,format_profile,extension_matches --view values file.docx
```
`extension_matches: false` means the container disagrees with the name. A renamed file usually exits
`4` (`MALFORMED_INPUT`) before it gets this far, which is itself the answer.

**Is it damaged?**
```bash
deckprobe -l d -t @quality --view values report.pdf
```
Use the selector, not `-t corrupted`: `corrupted` and `missing_assets` are declared for every modern
format but only the iWork drivers implement them, so naming them on a PDF or OOXML file exits `1`
with `UNSUPPORTED_TARGET`.

`@quality` gives you what the driver actually supports — `repaired` for PDF (`true` means the bounded
xref recovery had to run, so the file is readable but not clean), `conformance` for OOXML, and
`corrupted`/`missing_assets` for iWork:

```bash
deckprobe -l d -t @quality --view values deck.key
```

## Counts

**How many slides / pages / sheets?**
```bash
deckprobe -t slide_count --view values deck.pptx
deckprobe -t page_count  --view values report.pdf     # not in @summary; ask by name
deckprobe -t sheet_count,sheet_names --view values book.xlsx
deckprobe -t slide_count --view values deck.key
```

**Everything structural in one pass**
```bash
deckprobe -t @structure --view values --pretty deck.pptx
```

**Keynote slide state** — needs deep, unlike the plain count:
```bash
deckprobe -l d -t hidden_slide_count,slides_with_notes_count,orientation --view values deck.key
```

## Security review

**One-shot risk sweep**
```bash
deckprobe -l d -t @security --pretty untrusted.docx
```

**Just the cheap questions**
```bash
deckprobe -t encrypted,has_macros,password_protected --view values book.xlsm
```
`encrypted` is `header` level, so this is close to free.

**Does this PDF carry active content?**
```bash
deckprobe -l d -t has_javascript,has_embedded_files,active_content_risk --view values doc.pdf
```

**Is it signed, and by how many?**
```bash
deckprobe -l d -t has_digital_signature,signature_count --view values contract.pdf
```

## Metadata

**Who made it and when?**
```bash
deckprobe -t title,author,application,created_at,modified_at --view values --pretty report.docx
```

A `null` value means the field is absent from the document, not that the probe failed. An *unresolved*
target — listed in `execution.unresolved_targets` — is the "probe could not answer" case.

## Bulk work

**Inventory a directory**
```bash
find . -type f \( -name '*.pdf' -o -name '*.pptx' -o -name '*.docx' \) \
  | python3 -c 'import sys,json; [print(json.dumps({"path":l.strip()})) for l in sys.stdin]' \
  | deckprobe --jsonl -t @summary
```
One compact JSON per line. Per-record errors do not stop the run; the process exits with the highest
per-record status.

**Same thing, values only, easy to aggregate**
```bash
... | deckprobe --jsonl -t slide_count,author --view values
```

## Piping and non-file input

**Probe a download without saving it**
```bash
curl -sL "$url" | deckprobe -n download.pdf -t @security -
```
`-n` supplies the logical filename — the extension is what selects the format, so it is required.

**Base64 payload through JSONL**
```bash
printf '{"name":"report.pdf","data_base64":"%s"}\n' "$(base64 < report.pdf)" \
  | deckprobe --jsonl -t page_count --view values
```

## CI and scripting

**Fail the build if a target cannot be answered**
```bash
deckprobe -s -t format,page_count report.pdf
```
`--strict` exits `5` on an unresolved target and still writes the full report.

**Estimate cost before paying it**
```bash
deckprobe -P -p -l d -t @all huge.pdf
```
`--plan-only` reports the paths that *would* run and their estimated cost without executing driver
probe paths.

**Shell completion**
```bash
deckprobe completion zsh > ~/.zfunc/_deckprobe
```

## Reading the output in a script

```bash
if ! report=$(deckprobe -t slide_count --view values deck.pptx); then
  echo "probe failed: $(printf '%s' "$report" | python3 -c 'import json,sys; print(json.load(sys.stdin)["error"]["message"])')" >&2
  exit 1
fi
printf '%s' "$report" | python3 -c 'import json,sys; print(json.load(sys.stdin)["values"]["powerpoint.slide_count"])'
```

Note `values` is keyed by the **canonical** target name even when the request used the short alias.
