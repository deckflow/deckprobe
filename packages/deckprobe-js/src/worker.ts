import { inputBytes, inputName } from "./index.js";
import type { ProbeCallOptions, ProbeInput, ProbeResult } from "./types.js";

interface WorkerRequest {
  id: number;
  name: string;
  input: Blob | ArrayBuffer;
  options: ProbeCallOptions;
}

interface SerializedWorkerError {
  name: string;
  message: string;
}

interface WorkerResponse {
  id: number;
  result?: ProbeResult;
  error?: SerializedWorkerError;
}

interface Pending {
  resolve: (result: ProbeResult) => void;
  reject: (error: Error) => void;
}

export class DeckProbeWorker {
  readonly #worker: Worker;
  readonly #pending = new Map<number, Pending>();
  #nextId = 1;
  #state: "active" | "failed" | "terminated" = "active";

  constructor(
    worker = new Worker(new URL("./worker-runtime.js", import.meta.url), {
      type: "module",
    }),
  ) {
    this.#worker = worker;
    this.#worker.addEventListener("message", (event: MessageEvent<WorkerResponse>) => {
      const pending = this.#pending.get(event.data.id);
      if (!pending) return;
      this.#pending.delete(event.data.id);
      if (event.data.error !== undefined) {
        const error =
          event.data.error.name === "TypeError"
            ? new TypeError(event.data.error.message)
            : new Error(event.data.error.message);
        error.name = event.data.error.name;
        pending.reject(error);
      } else if (event.data.result) {
        pending.resolve(event.data.result);
      } else {
        pending.reject(new Error("DeckProbe worker returned an empty response"));
      }
    });
    this.#worker.addEventListener("error", (event) => {
      this.#fail(new Error(event.message || "DeckProbe worker failed"));
    });
    this.#worker.addEventListener("messageerror", () => {
      this.#fail(new Error("DeckProbe worker message could not be decoded"));
    });
  }

  async probe(input: ProbeInput, options: ProbeCallOptions = {}): Promise<ProbeResult> {
    this.#assertActive();
    const name = inputName(input, options.name);
    const isBlob = typeof Blob !== "undefined" && input instanceof Blob;
    const workerInput = isBlob ? input : (await inputBytes(input)).slice().buffer;
    this.#assertActive();
    const id = this.#nextId++;
    const request: WorkerRequest = {
      id,
      name,
      input: workerInput,
      options,
    };
    const result = new Promise<ProbeResult>((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
    });
    try {
      if (workerInput instanceof ArrayBuffer) {
        this.#worker.postMessage(request, [workerInput]);
      } else {
        this.#worker.postMessage(request);
      }
    } catch (error) {
      this.#pending.delete(id);
      throw error;
    }
    return result;
  }

  terminate(): void {
    if (this.#state === "terminated") return;
    this.#state = "terminated";
    this.#worker.terminate();
    this.#rejectPending(new Error("DeckProbe worker terminated"));
  }

  #assertActive(): void {
    if (this.#state !== "active") {
      throw new Error(`DeckProbe worker is ${this.#state}`);
    }
  }

  #fail(error: Error): void {
    if (this.#state !== "active") return;
    this.#state = "failed";
    this.#worker.terminate();
    this.#rejectPending(error);
  }

  #rejectPending(error: Error): void {
    for (const pending of this.#pending.values()) pending.reject(error);
    this.#pending.clear();
  }
}

export function createDeckProbeWorker(): DeckProbeWorker {
  return new DeckProbeWorker();
}
