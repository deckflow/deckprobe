# Target catalogue

The authoritative list always comes from the binary:

```bash
deckprobe targets --format pptx --pretty
```

That returns, for every target: canonical `id`, `aliases`, `min_level`, `value_type`, a JSON Schema
fragment, `cost_class`, selector membership, plus the profile's `format_options` and the complete
`selector_expansions` for `header`, `metadata`, and `deep`. This file is a readable mirror — when the
two disagree, the command wins.

Accepted format names: `pdf`, `word`/`docx`/`docm`, `excel`/`xlsx`/`xlsm`, `powerpoint`/`pptx`/`pptm`,
`keynote`/`key`, `numbers`, `pages`, and `legacy`/`doc`/`xls`/`ppt`.

Use the short alias on the command line; reports key `results` by the canonical id.

## Shared across every modern format

These are available for PDF, OOXML, and iWork alike. `min_level` in parentheses.

**Identity** — `format`, `format_profile` (alias `profile`), `mime_type` (alias `mime`), `file_size`,
`extension`, `extension_matches`. All `header`, all cheap.

**Metadata** (all `metadata`) — `title`, `subject`, `author`, `keywords`, `description`, `created_at`,
`modified_at`, `application`, `application_version`, `language`, `locale`. Each is nullable; a null
value means the field is genuinely absent, not that the probe failed.

**Security**

| Alias | Level | Type | Notes |
| --- | --- | --- | --- |
| `encrypted` | header | bool | Cheapest security signal there is |
| `has_macros` (`macros`) | metadata | bool | |
| `password_protected` | metadata | bool\|null | |
| `has_digital_signature` | metadata | bool | |
| `signature_count` | deep | u64\|null | |
| `has_javascript` | deep | bool | |
| `has_external_relationships` | metadata | bool | |
| `has_embedded_files` | metadata | bool | |
| `active_content_risk` | deep | string\|null | Rolled-up verdict |

**Quality** — `corrupted` and `missing_assets` (both deep, bool|null) are declared for every modern
format, but only the iWork drivers currently implement a path for them. On PDF and OOXML they return
`UNSUPPORTED_TARGET` (exit `1`), so use the `@quality` selector instead, which resolves to whatever
that driver really supports:

| Format | `@quality` resolves to |
| --- | --- |
| PDF | `extension_matches`, `repaired` |
| docx / xlsx / pptx | `extension_matches`, `conformance` |
| key / numbers / pages | `extension_matches`, plus `corrupted` and `missing_assets` at `-l d` |

This is the general rule, not a quirk: a target listed by `deckprobe targets` is declared for the
format, not guaranteed to have an executable path. Prefer a selector when you want "whatever this
driver can tell me", and name a target directly only when you need that specific fact and can handle
exit `1`.

## PDF

| Alias | Level | Type |
| --- | --- | --- |
| `version` | header | string |
| `linearized` | header | bool |
| `page_count` | metadata | u64\|null |
| `object_count` | metadata | u64\|null |
| `xref_type` | metadata | string |
| `repaired` | metadata | bool |
| `annotation_count` | metadata | u64\|null |
| `form_field_count` | metadata | u64\|null |
| `attachment_count` | metadata | u64\|null |
| `has_xmp` | metadata | bool |

`page_count` is **not** in `@summary` — the driver can only reach it through a path `@summary`
excludes. Ask for it by name or use `@structure`.

Format options: `pdf.repair_xref` (`safe` default, or `none`) and `pdf.max_objects` (default
`100000`).

## OOXML — shared by docx / xlsx / pptx

`document_kind` (header, string), `package_entry_count` (metadata, u64), `conformance`
(metadata, string).

### Word (docx, docm, dotx, dotm)

`page_count`, `word_count`, `character_count`, `paragraph_count`, `is_template`,
`unique_image_asset_count`, `comment_part_count` — all `metadata`. `table_count` is `deep`.

Counts come from the package's own statistics, so `page_count` reflects what the authoring
application last recorded, not a re-layout.

### Excel (xlsx, xlsm, xltx, xltm, xlsb)

`sheet_count`, `sheet_names`, `hidden_sheet_count`, `defined_name_count`, `table_count`,
`is_template`, `binary_workbook`, `chart_part_count`, `pivot_table_part_count`,
`unique_image_asset_count` — all `metadata`. `shared_string_count` is `deep`.

`.xlsb` is identity-only: it routes and validates, but the structural targets stay unresolved.

Format option: `excel.workbook_path` (default `auto`).

### PowerPoint (pptx, pptm, ppsx, ppsm, potx, potm)

All `metadata`: `slide_count`, `hidden_slide_count`, `master_count`, `layout_count`,
`notes_slide_count` (alias `notes_count`), `slide_size`, `aspect_ratio`, `orientation`,
`presentation_kind`, `chart_part_count`, `unique_image_asset_count`, `unique_media_asset_count`,
`comment_part_count`.

## iWork — shared by key / numbers / pages

`document_kind` (header). At `metadata`: `file_format_version`, `producer_build`,
`package_entry_count`, `iwa_entry_count`, `data_asset_count`, `data_asset_bytes`,
`asset_type_counts`, `has_preview`, `preview_count`, `preview_dimensions`, `is_multi_page`,
`has_external_or_missing_data`. At `deep`: `all_iwa_valid`, `archive_object_count`,
`message_type_counts`, `object_type_counts`.

Validation requires `Index/Document.iwa`, `Metadata/Properties.plist`, and the expected IWA root
object family. Legacy XML iWork returns `UNSUPPORTED_FORMAT`.

### Keynote

`metadata`: `slide_count`, `master_slide_count`, `table_component_count`.
`deep`: `slide_size`, `aspect_ratio`, `orientation`, `hidden_slide_count`, `slides_with_notes_count`,
`slides_with_builds_count`, `slides_with_transitions_count`, `table_count`.

Note the split — a plain slide count is cheap, but anything about slide *state* needs `-l d`.

### Numbers

`metadata`: `sheet_count`, `sheet_names`, `table_component_count`.
`deep`: `table_count`, `table_dimensions`, `hidden_row_count`, `hidden_column_count`,
`filtered_row_count`, `formula_definition_count`.

### Pages

`metadata`: `table_component_count`.
`deep`: `section_count`, `section_names`, `page_size`, `aspect_ratio`, `orientation`,
`change_tracking_enabled`, `body_text_length`, `body_paragraph_break_count`, `cached_page_count`,
`table_count`.

## Legacy Office (doc, xls, ppt)

Metadata and format statistics only — there is no deep content path, and the shared
`document.*`/`security.*`/`quality.*` families do not apply.

`header`: `document_kind`, `legacy_kind`, `cfb_container`, `content_probe_supported`.
`metadata`: `cfb_entry_count`.

- `.doc` adds `page_count`, `word_count`, `character_count`, `paragraph_count`, `is_template`.
- `.xls` adds `sheet_count`, `sheet_names`, `is_template`, `binary_workbook`.
- `.ppt` adds `slide_count`, `notes_slide_count`, `presentation_kind`.

Check `content_probe_supported` before trusting the absence of a structural value.

## Optional targets

`-o` requests targets that are returned only when a path already selected for a required target
produces them for free. They never add a path and never make the report `partial`:

```bash
deckprobe -t page_count -o object_count report.pdf
```

`-N` / `--no-piggyback` disables that zero-cost collection.

## Per-target confidence

`-C target=level` overrides `-c` for one target. A short alias works when unambiguous:

```bash
deckprobe -t slide_count,orientation -C slide_count=exact deck.pptx
```
