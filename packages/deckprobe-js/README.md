# @deckflow/deckprobe

Browser SDK for DeckProbe 2.2. Documents stay in the browser and are passed to
the shared Rust engine as bytes; the package does not upload files or use a
filesystem API.

```sh
npm install @deckflow/deckprobe
```

## Public imports

| Need | Import |
|---|---|
| Probe on the calling thread | `import { initDeckProbe, probe } from "@deckflow/deckprobe"` |
| Discover supported formats, targets, schema, or runtime version | `import { formats, targets, schema, version } from "@deckflow/deckprobe"` |
| Probe in a module Worker | `import { createDeckProbeWorker } from "@deckflow/deckprobe/worker"` |
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

`Uint8Array`, `ArrayBuffer`, and `Blob` inputs require `options.name` with a
filename extension. `File` inputs use `File.name` automatically. Successful and
partial probes return the same schema-v2 report as the native CLI; recognized
probe failures return the schema-v2 error envelope.

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
build artifacts and are intentionally ignored by Git.

The package is published independently as `@deckflow/deckprobe` while its Rust
binding remains part of the main DeckProbe Cargo workspace.
