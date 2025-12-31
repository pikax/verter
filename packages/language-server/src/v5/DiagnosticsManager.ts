import {
  CancellationToken,
  CancellationTokenSource,
  Connection,
  Diagnostic,
} from "vscode-languageserver";
import { VerterManager } from "./documents/verter/manager";
import { DocumentManager, VerterDocument, VueDocument } from "./documents";
import { VueSubDocument } from "./documents/verter/vue/sub/sub";
import {
  VueStyleDocument,
  VueTypescriptDocument,
} from "./documents/verter/vue/sub";
import { mapDiagnostic } from "./helpers";

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

const BATCH_DELAY = 10; // ms

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
    protected documentManager: DocumentManager
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

      // if (request.version !== request.doc.version) {
      //   console.warn(
      //     `[diagnostics] Skipping diagnostics for ${request.doc.uri} - document version has changed (requested: ${request.version}, current: ${request.doc.version})`
      //   );
      //   // document has changed, skip
      //   continue;
      // }
      this.requests.delete(request.doc.uri);

      const result = this.retrieveDiagnostics(request.doc, token);
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

  private retrieveDiagnostics(
    doc: VerterDocument,
    token: CancellationToken
  ): DiagnosticProcessingResult | null {
    const docs = doc instanceof VueDocument ? doc.docs : null;
    if (!docs) {
      return null;
    }

    const diagnostics = docs.flatMap(
      (d) => this.getDocDiagnostics(d, token) ?? []
    );
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

      console.time("diag");
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
      console.timeEnd("diag");

      return r;
    } else if (doc instanceof VueStyleDocument) {
      if (!doc.languageService) {
        console.warn(
          `[diagnostics] No language service for style document: ${doc.uri}`
        );
        return null;
      }
      return doc.languageService.doValidation(doc, doc.stylesheet, {
        validate: true,
      });
    }

    return null;
  }
}
