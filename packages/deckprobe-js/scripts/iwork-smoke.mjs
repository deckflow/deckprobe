import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { initDeckProbe, probe, targets } from "../dist/index.js";

const encoder = new TextEncoder();

function concat(...parts) {
  const length = parts.reduce((total, part) => total + part.length, 0);
  const output = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

function le16(value) {
  return Uint8Array.of(value & 0xff, (value >>> 8) & 0xff);
}

function le32(value) {
  return Uint8Array.of(
    value & 0xff,
    (value >>> 8) & 0xff,
    (value >>> 16) & 0xff,
    (value >>> 24) & 0xff,
  );
}

const crcTable = Array.from({ length: 256 }, (_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit += 1) {
    value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  }
  return value >>> 0;
});

function crc32(bytes) {
  let value = 0xffffffff;
  for (const byte of bytes) value = crcTable[(value ^ byte) & 0xff] ^ (value >>> 8);
  return (value ^ 0xffffffff) >>> 0;
}

function storedZip(entries) {
  const localParts = [];
  const centralParts = [];
  let localOffset = 0;
  for (const [name, data] of entries) {
    const nameBytes = encoder.encode(name);
    const checksum = crc32(data);
    const local = concat(
      le32(0x04034b50),
      le16(20),
      le16(0),
      le16(0),
      le16(0),
      le16(0),
      le32(checksum),
      le32(data.length),
      le32(data.length),
      le16(nameBytes.length),
      le16(0),
      nameBytes,
      data,
    );
    localParts.push(local);
    centralParts.push(
      concat(
        le32(0x02014b50),
        le16(20),
        le16(20),
        le16(0),
        le16(0),
        le16(0),
        le16(0),
        le32(checksum),
        le32(data.length),
        le32(data.length),
        le16(nameBytes.length),
        le16(0),
        le16(0),
        le16(0),
        le16(0),
        le32(0),
        le32(localOffset),
        nameBytes,
      ),
    );
    localOffset += local.length;
  }
  const central = concat(...centralParts);
  return concat(
    ...localParts,
    central,
    le32(0x06054b50),
    le16(0),
    le16(0),
    le16(entries.length),
    le16(entries.length),
    le32(central.length),
    le32(localOffset),
    le16(0),
  );
}

function varint(input) {
  let value = input;
  const output = [];
  do {
    let byte = value & 0x7f;
    value = Math.floor(value / 128);
    if (value) byte |= 0x80;
    output.push(byte);
  } while (value);
  return Uint8Array.from(output);
}

function fieldVarint(number, value) {
  return concat(varint(number << 3), varint(value));
}

function fieldBytes(number, value) {
  return concat(varint((number << 3) | 2), varint(value.length), value);
}

function fieldFixed32(number, value) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setFloat32(0, value, true);
  return concat(varint((number << 3) | 5), bytes);
}

function reference(identifier) {
  return fieldVarint(1, identifier);
}

function iwaObjects(objects) {
  const frames = objects.map(({ identifier, messageType, payload }) => {
    const messageInfo = concat(
      fieldVarint(1, messageType),
      fieldVarint(3, payload.length),
    );
    const archiveInfo = concat(
      fieldVarint(1, identifier),
      fieldBytes(2, messageInfo),
    );
    return concat(varint(archiveInfo.length), archiveInfo, payload);
  });
  const stream = concat(...frames);
  return concat(
    Uint8Array.of(1, stream.length & 0xff, (stream.length >>> 8) & 0xff, 0),
    stream,
  );
}

function modernKeynote() {
  const properties = encoder.encode(`<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>fileFormatVersion</key><string>14.4.1</string>
<key>isMultiPage</key><true/>
<key>hasExternalReferenceOrMissingOrUnmaterializedRemoteData</key><false/>
<key>language</key><string>en</string>
<key>locale</key><string>en_US</string>
</dict></plist>`);
  const buildHistory = encoder.encode(`<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><array>
<string>Template: White (13.2)</string>
<string>M14.4-7043.0.93-4</string>
</array></plist>`);
  const preview = Uint8Array.from([
    0xff, 0xd8, 0xff, 0xc0, 0x00, 0x07, 0x08, 0x02, 0x40, 0x04, 0x00,
  ]);
  const pngHeader = concat(
    Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    new Uint8Array(8),
    Uint8Array.from([0, 0, 0, 16, 0, 0, 0, 16]),
  );
  const showSize = concat(fieldFixed32(1, 1920), fieldFixed32(2, 1080));
  const show = fieldBytes(4, showSize);
  const slideNode = concat(
    fieldBytes(2, reference(100)),
    fieldVarint(4, 1),
    fieldVarint(5, 0),
    fieldVarint(6, 1),
    fieldVarint(7, 1),
    fieldVarint(8, 1),
  );
  return storedZip([
    [
      "Index/Document.iwa",
      iwaObjects([
        { identifier: 1, messageType: 1, payload: new Uint8Array() },
        { identifier: 2, messageType: 2, payload: show },
      ]),
    ],
    [
      "Index/Slide.iwa",
      iwaObjects([{ identifier: 3, messageType: 4, payload: slideNode }]),
    ],
    ["Metadata/Properties.plist", properties],
    ["Metadata/BuildVersionHistory.plist", buildHistory],
    ["Data/photo.png", pngHeader],
    ["preview.jpg", preview],
  ]);
}

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const wasm = readFileSync(resolve(packageDirectory, "wasm/deckprobe_wasm_bg.wasm"));

await initDeckProbe(wasm);
const report = await probe(modernKeynote(), {
  name: "synthetic.key",
  level: "deep",
  targets: ["@all"],
});

assert.equal(report.status, "ok", JSON.stringify(report));
if (report.status === "error") throw new Error(JSON.stringify(report));
assert.equal(report.input.source_kind, "browser_bytes");
assert.equal(report.results["iwork.producer_build"].value, "M14.4-7043.0.93-4");
assert.equal(report.results["document.language"].value, "en");
assert.equal(report.results["document.locale"].value, "en_US");
assert.equal(report.results["iwork.has_external_or_missing_data"].value, false);
assert.equal(report.results["iwork.data_asset_bytes"].value, 24);
assert.equal(report.results["iwork.asset_type_counts"].value.image, 1);
assert.deepEqual(report.results["iwork.preview_dimensions"].value["preview.jpg"], {
  height: 576,
  width: 1024,
});
assert.equal(report.results["iwork.all_iwa_valid"].value, true);
assert.equal(report.results["iwork.archive_object_count"].value, 3);
assert.equal(report.results["iwork.message_type_counts"].value["4"], 1);
assert.equal(report.results["keynote.slide_size"].value.width_pt, 1920);
assert.equal(report.results["keynote.aspect_ratio"].value.decimal, 16 / 9);
assert.equal(report.results["keynote.orientation"].value, "landscape");
assert.equal(report.results["keynote.hidden_slide_count"].value, 1);
assert.equal(report.results["keynote.slides_with_notes_count"].value, 1);
assert.equal(report.results["keynote.slides_with_builds_count"].value, 1);
assert.equal(report.results["keynote.slides_with_transitions_count"].value, 1);
assert.equal(report.results["quality.corrupted"].value, false);

const discovery = await targets("key");
assert.equal(discovery.status, "ok", JSON.stringify(discovery));
if (discovery.status === "error") throw new Error(JSON.stringify(discovery));
const producerBuild = discovery.targets.find(
  (target) => target.id === "iwork.producer_build",
);
assert.equal(producerBuild?.applicable, true);
assert.deepEqual(producerBuild?.supported_levels, ["metadata", "deep"]);
const slideSize = discovery.targets.find(
  (target) => target.id === "keynote.slide_size",
);
assert.equal(slideSize?.applicable, true);
assert.deepEqual(slideSize?.supported_levels, ["deep"]);

const numbersDiscovery = await targets("numbers");
assert.equal(numbersDiscovery.status, "ok", JSON.stringify(numbersDiscovery));
if (numbersDiscovery.status === "error") {
  throw new Error(JSON.stringify(numbersDiscovery));
}
const tableDimensions = numbersDiscovery.targets.find(
  (target) => target.id === "numbers.table_dimensions",
);
assert.equal(tableDimensions?.applicable, true);
assert.deepEqual(tableDimensions?.supported_levels, ["deep"]);

const pagesDiscovery = await targets("pages");
assert.equal(pagesDiscovery.status, "ok", JSON.stringify(pagesDiscovery));
if (pagesDiscovery.status === "error") {
  throw new Error(JSON.stringify(pagesDiscovery));
}
const bodyTextLength = pagesDiscovery.targets.find(
  (target) => target.id === "pages.body_text_length",
);
assert.equal(bodyTextLength?.applicable, true);
assert.deepEqual(bodyTextLength?.supported_levels, ["deep"]);

console.log("iWork JS SDK smoke passed: synthetic Keynote deep @all and discovery");
