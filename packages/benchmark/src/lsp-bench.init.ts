/**
 * Shared LSP `initialize` routine for the benchmark runners.
 *
 * Both the Verter and Volar benchmark paths must complete the LSP 3.17
 * position-encoding handshake: advertise `general.positionEncodings` and adopt
 * the server's chosen `positionEncoding`. Routing every benchmark's init through
 * {@link LspClient.initialize} keeps that negotiation in one place — a raw
 * `initialize` request would silently leave the client at the UTF-16 default and
 * never tell the server which encodings it accepts.
 */
import type { LspClient } from "@verter/lsp-test-client";

/** Build the `initialize` params shared by the Verter and Volar benchmarks. */
export function makeInitializeParams(
  rootUri: string,
  workspaceName: string,
  initializationOptions?: unknown,
) {
  return {
    processId: process.pid,
    capabilities: {
      textDocument: {
        publishDiagnostics: {
          relatedInformation: true,
        },
        hover: {
          contentFormat: ["markdown", "plaintext"],
        },
        completion: {
          completionItem: {
            snippetSupport: false,
          },
        },
      },
      workspace: {
        workspaceFolders: true,
      },
    },
    rootUri,
    workspaceFolders: [
      {
        uri: rootUri,
        name: workspaceName,
      },
    ],
    ...(initializationOptions ? { initializationOptions } : {}),
  };
}

/**
 * Run the benchmark client's `initialize` through {@link LspClient.initialize}
 * so it advertises `general.positionEncodings` and adopts the server's
 * negotiated `positionEncoding`. Returns the raw `InitializeResult`.
 */
export function initializeBenchmarkClient<T = unknown>(
  client: LspClient,
  rootUri: string,
  workspaceName: string,
  timeout: number,
  initializationOptions?: unknown,
): Promise<T> {
  return client.initialize<T>(
    makeInitializeParams(rootUri, workspaceName, initializationOptions),
    timeout,
  );
}
