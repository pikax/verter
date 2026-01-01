import {
  CancellationToken,
  CancellationTokenSource,
  Connection,
  Diagnostic,
} from "vscode-languageserver";
import { performance } from "node:perf_hooks";
import { VerterManager } from "./documents/verter/manager";
import { DocumentManager, VerterDocument, VueDocument } from "./documents";
import { VueSubDocument } from "./documents/verter/vue/sub/sub";
import {
  VueStyleDocument,
  VueTypescriptDocument,
  VueRenderDocument,
  VueBundleDocument,
} from "./documents/verter/vue/sub";
import { mapDiagnostic } from "./helpers";
import { StatisticsManager } from "./StatisticsManager";

type DiagnosticRequest = {
  version: number;
  doc: VerterDocument;

  tokenSource: CancellationTokenSource;
};

type DiagnosticResult = {
  uri: string;
  diagnostics: Diagnostic[];
};

type DiagnosticProcessingResult = DiagnosticResult & {
  token: CancellationToken;
};

const BATCH_DELAY = 250; // ms

function debounce<T extends (...args: any[]) => void>(
  fn: T,
  delay: number
): (...args: Parameters<T>) => void {
  let t: ReturnType<typeof setTimeout>;
  return (...args) => {
    clearTimeout(t);
    t = setTimeout(() => fn(...args), delay);
  };
}

export class DiagnosticsManager {
  requests: Map<string, DiagnosticRequest> = new Map();
  batchRequests: DiagnosticRequest[] = [];
  batchResults: DiagnosticProcessingResult[] = [];
  private processBatch: () => void;
  private processBatchResults: () => void;

  constructor(
    protected connection: Connection,
    protected verterManager: VerterManager,
    protected documentManager: DocumentManager,
    private readonly statistics?: StatisticsManager
  ) {
    this.processBatch = debounce(
      this.processBatchInternal.bind(this),
      BATCH_DELAY
    );
    this.processBatchResults = debounce(
      this.processBatchResultsInternal.bind(this),
      BATCH_DELAY
    );
  }

  requestDiagnostics(documentUri: string) {
    this.cancelPreviousRequest(documentUri);

    const doc = this.documentManager.getDocument(documentUri);
    if (!doc) {
      console.warn(
        `[diagnostics] Diagnostics requested for unknown document: ${documentUri}`
      );
      return;
    }

    try {
      if (!(doc instanceof VueDocument)) {
        return;
      }

      const token = new CancellationTokenSource();
      const request: DiagnosticRequest = {
        doc,
        tokenSource: token,
        version: doc.version,
      };
      this.addToBatch(request);
    } catch (e) {
      console.error("error [diagnostics]", e, VueDocument);
    }
  }

  private addToBatch(request: DiagnosticRequest) {
    this.requests.set(request.doc.uri, request);
    this.batchRequests.push(request);
    this.processBatch();
  }

  private batchDiagnosticResult(result: DiagnosticProcessingResult | null) {
    if (!result) return;
    this.batchResults.push(result);
    this.processBatchResults();
  }

  private processBatchInternal() {
    while (this.batchRequests.length) {
      const request = this.batchRequests.shift()!;
      const token = request.tokenSource.token;
      // process request

      if (token.isCancellationRequested) {
        this.requests.delete(request.doc.uri);
        continue;
      }

      if (request.version !== request.doc.version) {
        // document has changed, skip
        this.requests.delete(request.doc.uri);
        continue;
      }

      // if (request.version !== request.doc.version) {
      //   console.warn(
      //     `[diagnostics] Skipping diagnostics for ${request.doc.uri} - document version has changed (requested: ${request.version}, current: ${request.doc.version})`
      //   );
      //   // document has changed, skip
      //   continue;
      // }
      this.requests.delete(request.doc.uri);

      const start = performance.now();
      const result = this.retrieveDiagnostics(request.doc, token);
      const durationMs = performance.now() - start;
      if (result) {
        this.statistics?.recordEvent({
          type: "diagnostics",
          uri: request.doc.uri,
          durationMs,
          meta: {
            version: request.version,
            docs: request.doc instanceof VueDocument ? request.doc.docs.length : undefined,
          },
        });
      }
      if (token.isCancellationRequested) {
        console.warn(
          `[diagnostics] Skipping sending diagnostics for ${request.doc.uri} - request was cancelled`
        );
        continue;
      }

      this.batchDiagnosticResult(result);
    }
  }

  private processBatchResultsInternal() {
    while (this.batchResults.length) {
      const { token, ...result } = this.batchResults.shift()!;
      if (token.isCancellationRequested) {
        console.warn(
          `[diagnostics] Skipping sending diagnostics for ${result.uri} - request was cancelled`
        );
        continue;
      }
      // send diagnostics
      this.connection.sendDiagnostics(result);
    }
  }

  private retrieveDiagnosticsSingle(
    doc: VerterDocument,
    token: CancellationToken
  ): DiagnosticProcessingResult | null {
    const docs = doc instanceof VueDocument ? doc.docs : null;
    if (!docs) {
      return null;
    }

    if (token.isCancellationRequested) {
      return null;
    }

    const tsDocs = docs.filter(
      (d): d is VueTypescriptDocument => d instanceof VueTypescriptDocument
    );
    const styleDocs = docs.filter(
      (d): d is VueStyleDocument => d instanceof VueStyleDocument
    );

    const primaryTsDoc =
      tsDocs.find((d) => d instanceof VueRenderDocument) ||
      tsDocs.find((d) => d instanceof VueBundleDocument) ||
      tsDocs[0];

    const docsToProcess = [primaryTsDoc, ...styleDocs].filter((d) => !!d);

    if (!docsToProcess.length) {
      return null;
    }

    const diagnostics = docsToProcess.flatMap(
      (d) => this.getDocDiagnostics(d, token) ?? []
    );
    if (token.isCancellationRequested) {
      return null;
    }
    return {
      uri: doc.uri,
      diagnostics: diagnostics,
      version: doc.version,
      token,
    } as DiagnosticProcessingResult;
  }

  private retrieveDiagnostics(
    doc: VerterDocument,
    token: CancellationToken
  ): DiagnosticProcessingResult | null {
    const docs = doc instanceof VueDocument ? doc.docs : null;
    if (!docs) {
      return null;
    }

    const sorted = [...docs].sort((x) => (x.uri.endsWith("tsx") ? 9 : 0));

    const diagnostics: Diagnostic[] = [];

    for (let i = 0; i < sorted.length; i++) {
      const d = sorted[i];
      if (token.isCancellationRequested) {
        return null;
      }

      // tsx file takes quite a bit to get diagnostics so we can just send new ones
      if (d.uri.endsWith("tsx")) {
        this.connection.sendDiagnostics({
          uri: doc.uri,
          diagnostics,
          version: doc.version,
        });
      }

      diagnostics.push(...(this.getDocDiagnostics(d, token) ?? []));
    }

    // const diagnostics = docs.flatMap(
    //   (d) => this.getDocDiagnostics(d, token) ?? []
    // );
    if (token.isCancellationRequested) {
      return null;
    }
    return {
      uri: doc.uri,
      diagnostics: diagnostics,
      token,
    } as DiagnosticProcessingResult;
  }

  private cancelPreviousRequest(documentUri: string) {
    const previousRequest = this.requests.get(documentUri);
    if (previousRequest) {
      previousRequest.tokenSource.cancel();
      previousRequest.tokenSource.dispose();
      this.requests.delete(documentUri);
    }
  }

  private getDocDiagnostics(doc: VueSubDocument, token: CancellationToken) {
    if (token.isCancellationRequested) {
      return null;
    }

    if (doc instanceof VueTypescriptDocument) {
      const tsService = this.verterManager.getTsService(doc.uri);
      if (!tsService) {
        console.error(`[diagnostics] No TS service for document: ${doc.uri}`);
        return null;
      }

      const start = performance.now();
      console.time("diag" + doc.uri);
      const r = [
        tsService.getSemanticDiagnostics,
        tsService.getSyntacticDiagnostics,
        tsService.getSuggestionDiagnostics,
      ]
        .flatMap((fn) => {
          if (token.isCancellationRequested) {
            return [];
          }
          return fn.call(tsService, doc.uri) ?? [];
        })
        .map((d) => mapDiagnostic(d, doc))
        .filter((d): d is Diagnostic => !!d);
      if (token.isCancellationRequested) {
        return null;
      }
      console.timeEnd("diag" + doc.uri);

      this.statistics?.recordEvent({
        type: "diagnostics:document",
        uri: doc.uri,
        durationMs: performance.now() - start,
        meta: { languageId: doc.languageId },
      });

      return r;
    } else if (doc instanceof VueStyleDocument) {
      if (!doc.languageService) {
        console.warn(
          `[diagnostics] No language service for style document: ${doc.uri}`
        );
        return null;
      }
      const start = performance.now();
      const diagnostics = doc.languageService.doValidation(
        doc,
        doc.stylesheet,
        {
          validate: true,
        }
      );
      this.statistics?.recordEvent({
        type: "diagnostics:style",
        uri: doc.uri,
        durationMs: performance.now() - start,
        meta: { languageId: doc.languageId },
      });
      return diagnostics;
    }

    return null;
  }
}
