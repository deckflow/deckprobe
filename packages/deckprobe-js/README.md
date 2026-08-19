# @deckflow/deckprobe

DeckProbe 2.2 for JavaScript: the `deckprobe` command line tool for Node, and a
browser SDK that runs the same Rust engine in WebAssembly. In the browser,
documents stay in the browser — the package does not upload files.

```sh
npm install @deckflow/deckprobe
```

## Command line

Installing the package puts `deckprobe` on `PATH`, or run it without installing:

```sh
npx @deckflow/deckprobe --help
npx @deckflow/deckprobe -t slide_count deck.pptx
```

This is the same native binary the standalone installers ship, so every flag,
help page, JSON report, and exit code matches the
[CLI reference](https://github.com/deckflow/deckprobe/blob/main/docs/CLI-REFERENCE.md)
exactly. The binary arrives through a per-platform optional dependency for the
targets DeckProbe releases: macOS arm64/x64, Linux arm64/x64 (glibc and musl),
and Windows x64.

If installation skipped optional dependencies, `deckprobe` reports which
platform package is missing. Install it directly, or build from source with
`cargo install --git https://github.com/deckflow/deckprobe --locked deckprobe`.

## Node API

The `node` export condition loads the WebAssembly binary from disk, so the
package works under Node with no configuration:

```ts
import { probeFile } from "@deckflow/deckprobe";

const report = await probeFile("deck.pptx", {
  targets: ["@summary", "@security"],
  level: "metadata",
});
```

`probeFile()` reports `source_kind: "local_file"` and produces a report byte for
byte identical to the native CLI on the same input. `probe()` accepts `Buffer`,
`Uint8Array`, and `ArrayBuffer` alongside the browser input types, and reports
`node_bytes`. `deckProbeWasmPath` exposes the absolute path of the binary being
loaded.

`probeFile()` reads the whole file into memory, because the WebAssembly engine
takes bytes rather than a file handle. The `deckprobe` command reads lazily
instead — for a 300 KB PPTX it touches under 18 KB — so prefer the CLI, or
`--jsonl` for batches, when inputs are large.

## Public imports

| Need | Import |
|---|---|
| Probe on the calling thread | `import { initDeckProbe, probe } from "@deckflow/deckprobe"` |
| Discover supported formats, targets, schema, or runtime version | `import { formats, targets, schema, version } from "@deckflow/deckprobe"` |
| Probe in a module Worker (browser only) | `import { createDeckProbeWorker } from "@deckflow/deckprobe/worker"` |
| Probe a file on disk (Node only) | `import { probeFile } from "@deckflow/deckprobe"` |
| Use TypeScript response and option types | `import type { ProbeResult, ProbeCallOptions } from "@deckflow/deckprobe"` |

`probe()` lazily initializes the WASM runtime. Use `initDeckProbe()` during
application startup or an idle period when the first interaction should avoid
that initialization cost.

## Choose an execution path

| Path | Choose it when |
|---|---|
| `probe()` on the main thread | The probe is small and its result directly drives an interaction. |
| `createDeckProbeWorker()` | Files are user-provided or potentially large, or `deep` inspection must not block rendering. |

Both paths run the same engine; the Worker adds byte-copy and messaging cost in
exchange for keeping the UI thread responsive. The first probe on either path
also pays one-time WASM initialization; see the
[execution modes](https://github.com/deckflow/deckprobe#execution-modes)
overview. Measure on your own corpus before optimizing.

Initialize on application startup or during idle time when you want the first
user interaction to use the warm path:

```ts
import { initDeckProbe, probe } from "@deckflow/deckprobe";

await initDeckProbe();

const report = await probe(file, {
  targets: ["@summary", "@security"],
  level: "metadata",
});
```

For larger documents, run the probe away from the UI thread. Keep the worker
alive across a batch, then terminate it with its owning component or job:

```ts
import { createDeckProbeWorker } from "@deckflow/deckprobe/worker";

const worker = createDeckProbeWorker();
try {
  const report = await worker.probe(file, { targets: ["@summary"] });
} finally {
  worker.terminate();
}
```

`terminate()` cancels all outstanding work and permanently closes that Worker
wrapper. Later `probe()` calls reject immediately; create a new wrapper for a
new batch. `File` and `Blob` reads happen inside the Worker, while direct
`probe()` intentionally runs input conversion and parsing on the calling
thread.

## Bundlers

The package ships a WebAssembly binary next to its JavaScript wrapper and
resolves it relative to that wrapper. Any tool that moves the wrapper without
moving the binary breaks initialization.

### Vite

Vite's dependency pre-bundling rewrites `@deckflow/deckprobe` into
`node_modules/.vite/deps/` and does not copy the adjacent `.wasm` file, so the
dev server requests a binary that is not there. **Every Vite version is
affected in dev; `vite build` is not affected** — a production build resolves
and emits the binary correctly. That asymmetry is the clearest way to recognize
the problem.

The reported error depends on how the dev server answers the missing path:

| Vite | Error |
|---|---|
| 4.x | `TypeError: Failed to execute 'compile' on 'WebAssembly': HTTP status code is not ok` |
| 5.x, 6.x, 7.x | `CompileError: WebAssembly.instantiate(): expected magic word 00 61 73 6d, found 3c 21 64 6f` |

On Vite 5 and newer the SPA fallback answers with `index.html`, so the request
succeeds and WebAssembly rejects the HTML body instead — `3c 21 64 6f` is
`<!do`. The Worker entry point fails the same way and surfaces it as
`DeckProbe worker failed to load`.

**Exclude the package from dependency pre-bundling.** This is the recommended
fix, and the only one that covers the Worker entry point:

```ts
// vite.config.ts
import { defineConfig } from "vite";

export default defineConfig({
  optimizeDeps: {
    exclude: ["@deckflow/deckprobe"],
  },
});
```

Excluding the package name also covers the `/worker` subpath; it does not need
its own entry.

**If you only use the main-thread entry point**, passing the binary URL works
without touching the Vite configuration:

```ts
import { initDeckProbe, probe } from "@deckflow/deckprobe";
import wasmUrl from "@deckflow/deckprobe/wasm?url";

await initDeckProbe(wasmUrl);
```

`?url` is Vite's asset syntax; add `/// <reference types="vite/client" />` so
TypeScript types the import. Call `initDeckProbe(wasmUrl)` before the first
`probe()` or discovery call, because the first initialization wins.

This form does not help `createDeckProbeWorker()`. The Worker runs its own
module instance, so a URL passed on the main thread never reaches it, and
pre-bundling relocates `worker-runtime.js` itself. Use `optimizeDeps.exclude`
whenever the Worker entry point is in play.

### Other bundlers

`@deckflow/deckprobe/wasm` is the stable path to the binary and accepts the
same treatment elsewhere: bundlers that understand `new URL(..., import.meta.url)`
(webpack 5, Rollup, esbuild) need no configuration, and anything else can load
the binary itself and hand it to `initDeckProbe()`:

```ts
await initDeckProbe(await fetch("/assets/deckprobe_wasm_bg.wasm"));
```

`initDeckProbe()` accepts a URL or path, a `Request` or `Response`, raw bytes,
or a compiled `WebAssembly.Module` (`InitInput`). Use the `Response` or bytes
form when a CDN origin, a sub-path deployment, or a strict CSP makes the
default relative URL wrong.

`Uint8Array`, `ArrayBuffer`, and `Blob` inputs require `options.name` with a
filename extension. `File` inputs use `File.name` automatically. Successful and
partial probes return the same schema-v2 report as the native CLI; recognized
probe failures return the schema-v2 error envelope.

A `status` of `"partial"` means one requested target could not be resolved; the
other results are still valid, and it is not a verdict on the document. Each
result carries `confidence` (`exact`, `high`, `medium`, `low`, `none`) describing
evidence strength, with `confidence_score` as a **fixed constant per label**
(`1.0`, `0.95`, `0.7`, `0.4`, `0.0`) rather than a calibrated probability — `0.95`
does not mean the value is right 95% of the time. See
[Reading confidence and partial results](https://github.com/deckflow/deckprobe/blob/main/docs/CLI-REFERENCE.md#reading-confidence-and-partial-results).

`input.source_kind` is typed as `SourceKind`: `browser_bytes` in the browser,
`node_bytes` for byte input under Node, `local_file` from `probeFile()` and the
native CLI, plus any custom value passed as `options.sourceKind`.

There is no `view` option. `--view values` is a CLI flag, so `probe()` and
`probeFile()` always resolve the full evidence report and `ProbeResult` is
`ProbeReport | ErrorReport`. If you parse `deckprobe --view values` output
instead, the exported `ValuesReport` type describes that envelope — note its
`unresolved_targets` sits at the top level rather than under `execution`.

Apple iWork files use the same selectors and target IDs as the native engine.
For example:

```ts
const report = await probe(file, {
  level: "deep",
  targets: [
    "iwork.producer_build",
    "iwork.asset_type_counts",
    "iwork.preview_dimensions",
    "iwork.archive_object_count",
    "iwork.message_type_counts",
    "keynote.slide_size",
    "keynote.hidden_slide_count",
    "iwork.all_iwa_valid",
  ],
});
```

`targets("key")`, `targets("numbers")`, and `targets("pages")` return typed
`TargetSpecReport` entries. Their `applicable` and `supported_levels` fields
distinguish the shared catalog from targets the chosen iWork driver can execute.

## Build

```sh
npm ci
npm run build
npx playwright install chromium
npm test
```

`npm run build` regenerates the Rust WASM package and then compiles the
TypeScript wrapper, so it requires `cargo`, the `wasm32-unknown-unknown` target,
and the local `wasm-pack` dependency. The generated files under `wasm/` are
build artifacts and are intentionally ignored by Git. Rebuild them after a
version bump: the smoke tests assert that the compiled runtime reports the same
version as `package.json`.

`npm test` runs the type check, the version and artifact checks, and three
browser suites. `npm run test:vite` is the bundler guard: it packs the real
tarball, installs it into a throwaway Vite consumer, and asserts that both
documented Vite fixes and a production build still reach the WASM binary. It
drives Vite through this package's own dependency, so it needs Node
`^20.19.0 || >=22.12.0` — the range Vite 7 requires.

The package is published independently as `@deckflow/deckprobe` while its Rust
binding remains part of the main DeckProbe Cargo workspace.
