import assert from "node:assert/strict";
import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import Ajv2020 from "ajv/dist/2020.js";
import { chromium } from "playwright";

const packageDirectory = resolve(fileURLToPath(new URL("..", import.meta.url)));
const contentTypes = {
  ".js": "text/javascript; charset=utf-8",
  ".map": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
};

function staticPath(requestUrl) {
  const pathname = decodeURIComponent(new URL(requestUrl, "http://localhost").pathname);
  const candidate = resolve(packageDirectory, `.${pathname}`);
  if (candidate !== packageDirectory && !candidate.startsWith(`${packageDirectory}${sep}`)) {
    return undefined;
  }
  return candidate;
}

const server = createServer((request, response) => {
  if (request.url === "/") {
    response.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    response.end("<!doctype html><title>DeckProbe browser smoke</title>");
    return;
  }
  const file = staticPath(request.url ?? "/");
  if (!file || !existsSync(file) || !statSync(file).isFile()) {
    response.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
    response.end("Not found");
    return;
  }
  response.writeHead(200, {
    "Cache-Control": "no-store",
    "Content-Type": contentTypes[extname(file)] ?? "application/octet-stream",
  });
  createReadStream(file).pipe(response);
});

await new Promise((resolveListening, rejectListening) => {
  server.once("error", rejectListening);
  server.listen(0, "127.0.0.1", resolveListening);
});

const address = server.address();
assert.ok(address && typeof address === "object", "smoke server did not bind");
const origin = `http://127.0.0.1:${address.port}`;
const browser = await chromium.launch({ headless: true });

try {
  const page = await browser.newPage();
  const browserErrors = [];
  page.on("pageerror", (error) => browserErrors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") browserErrors.push(message.text());
  });
  await page.goto(origin, { waitUntil: "domcontentloaded" });

  const outcome = await page.evaluate(async () => {
    function minimalPdf() {
      const objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 72 72] /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\n\nendstream",
      ];
      let source = "%PDF-1.4\n";
      const offsets = [0];
      for (const [index, object] of objects.entries()) {
        offsets.push(source.length);
        source += `${index + 1} 0 obj\n${object}\nendobj\n`;
      }
      const xref = source.length;
      source += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
      source += offsets
        .slice(1)
        .map((offset) => `${String(offset).padStart(10, "0")} 00000 n \n`)
        .join("");
      source += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\n`;
      source += `startxref\n${xref}\n%%EOF\n`;
      return new TextEncoder().encode(source);
    }

    const main = await import("/dist/index.js");
    const workerApi = await import("/dist/worker.js");
    const bytes = minimalPdf();
    const options = { targets: ["pdf.page_count"], level: "metadata" };
    const directReports = [];
    for (const [input, callOptions] of [
      [new File([bytes], "minimal.pdf"), options],
      [new Blob([bytes]), { ...options, name: "minimal.pdf" }],
      [bytes.buffer.slice(0), { ...options, name: "minimal.pdf" }],
      [bytes, { ...options, name: "minimal.pdf" }],
    ]) {
      directReports.push(await main.probe(input, callOptions));
    }

    let directTypeError;
    try {
      await main.probe(bytes, { name: "minimal.pdf", level: "invalid" });
    } catch (error) {
      directTypeError = { name: error?.name, message: error?.message };
    }

    const malformed = await main.probe(new Uint8Array([1, 2, 3, 4]), {
      name: "broken.pdf",
      targets: ["@header"],
      level: "header",
    });

    const worker = workerApi.createDeckProbeWorker();
    const workerReports = await Promise.all([
      worker.probe(new File([bytes], "minimal.pdf"), options),
      worker.probe(new Blob([bytes]), { ...options, name: "minimal.pdf" }),
    ]);

    let workerTypeError;
    try {
      await worker.probe(bytes, { name: "minimal.pdf", level: "invalid" });
    } catch (error) {
      workerTypeError = { name: error?.name, message: error?.message };
    }

    worker.terminate();
    let terminatedError;
    try {
      await worker.probe(bytes, { ...options, name: "minimal.pdf" });
    } catch (error) {
      terminatedError = { name: error?.name, message: error?.message };
    }

    return {
      directReports,
      directTypeError,
      malformed,
      schema: await main.schema(),
      version: await main.version(),
      workerReports,
      workerTypeError,
      terminatedError,
    };
  });

  assert.deepEqual(browserErrors, [], "browser emitted unexpected errors");
  assert.equal(outcome.version, "2.2.0");
  assert.equal(outcome.directTypeError?.name, "TypeError");
  assert.equal(outcome.workerTypeError?.name, "TypeError");
  assert.match(outcome.terminatedError?.message ?? "", /terminated/);
  assert.equal(outcome.malformed.status, "error");
  assert.equal(outcome.malformed.error.code, "MALFORMED_INPUT");

  const validate = new Ajv2020({ allErrors: true, strict: false }).compile(outcome.schema);
  for (const report of [...outcome.directReports, ...outcome.workerReports, outcome.malformed]) {
    assert.equal(validate(report), true, JSON.stringify(validate.errors));
    if (report.status !== "error") {
      assert.equal(report.input.source_kind, "browser_bytes");
      assert.equal(report.results["pdf.page_count"].value, 1);
    }
  }

  console.log(
    `Browser smoke passed: ${outcome.directReports.length} inputs, ` +
      `${outcome.workerReports.length} Worker probes, schema-v2 validated`,
  );
} finally {
  await browser.close();
  await new Promise((resolveClosed, rejectClosed) => {
    server.close((error) => (error ? rejectClosed(error) : resolveClosed()));
  });
}
