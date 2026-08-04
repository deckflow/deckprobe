import { probe } from "./index.js";
import type { ProbeCallOptions, ProbeResult } from "./types.js";

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

interface WorkerScope {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
  postMessage(message: WorkerResponse): void;
}

const scope = globalThis as unknown as WorkerScope;

scope.onmessage = async (event) => {
  const { id, name, input, options } = event.data;
  try {
    const probeInput = input instanceof Blob ? input : new Uint8Array(input);
    const result = await probe(probeInput, { ...options, name });
    scope.postMessage({ id, result });
  } catch (error) {
    scope.postMessage({
      id,
      error: {
        name: error instanceof Error ? error.name : "Error",
        message: error instanceof Error ? error.message : String(error),
      },
    });
  }
};
