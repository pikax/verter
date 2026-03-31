import { createRequire } from "node:module";

import {
  normalizeForBenchmark,
  type MetaUiBackend,
  type MetaUiOutcomeBucket,
  type NormalizedMetaArtifact,
} from "./meta-ui-core.js";
import { propsToJsonSchema, refineMetaForBenchmark } from "./meta-ui-meta.js";
import { loadVerterCompatModule } from "./verter-compat.js";

interface PreparedComponentSnapshot {
  absolutePath: string;
  relativePath: string;
  transformedSource: string;
}

interface WorkerInitPayload {
  backend: MetaUiBackend;
  uiRoot: string;
  checkerConfig: Record<string, unknown>;
  components: PreparedComponentSnapshot[];
}

interface MeasuredQueryResult {
  artifact: NormalizedMetaArtifact;
  latencyMs: number;
  outcome: MetaUiOutcomeBucket;
}

type ParentMessage =
  | { type: "init"; payload: WorkerInitPayload }
  | { type: "query"; requestId: number; component: PreparedComponentSnapshot };

const require = createRequire(import.meta.url);

let checkerPromise: Promise<any> | null = null;

process.on("message", (message: ParentMessage) => {
  void handleMessage(message);
});

async function handleMessage(message: ParentMessage): Promise<void> {
  if (message.type === "init") {
    try {
      const checker = await createChecker(
        message.payload.uiRoot,
        message.payload.checkerConfig,
        message.payload.backend,
      );
      for (const component of message.payload.components) {
        checker.updateFile(component.absolutePath, component.transformedSource);
      }
      checkerPromise = Promise.resolve(checker);
      process.send?.({ type: "ready" });
    } catch (error) {
      process.send?.({
        type: "fatal",
        message: error instanceof Error ? error.message : String(error),
        stack: error instanceof Error ? error.stack : undefined,
      });
    }
    return;
  }

  if (message.type === "query") {
    try {
      if (!checkerPromise) {
        throw new Error("meta-ui benchmark worker was queried before initialization");
      }
      const checker = await checkerPromise;
      const result = await executeMeasuredQuery(checker, message.component);
      process.send?.({ type: "result", requestId: message.requestId, result });
    } catch (error) {
      process.send?.({
        type: "error",
        requestId: message.requestId,
        message: error instanceof Error ? error.message : String(error),
        stack: error instanceof Error ? error.stack : undefined,
      });
    }
  }
}

async function createChecker(
  uiRoot: string,
  checkerConfig: Record<string, unknown>,
  backend: MetaUiBackend,
): Promise<any> {
  if (backend === "vue-component-meta") {
    const module = require("vue-component-meta");
    if (typeof module.createCheckerByJson === "function") {
      return module.createCheckerByJson(uiRoot, checkerConfig, {
        forceUseTs: true,
        schema: true,
      });
    }
    throw new Error("Installed vue-component-meta does not expose createCheckerByJson().");
  }

  const { createCheckerByJson } = await loadVerterCompatModule();
  return createCheckerByJson(uiRoot, checkerConfig, {
    forceUseTs: true,
    schema: { literalBooleanSchema: true },
    runtimeMode: "dedicated",
    typeExpansionBackend: backend === "verter" ? "verter" : backend,
  });
}

async function executeMeasuredQuery(
  checker: any,
  component: PreparedComponentSnapshot,
): Promise<MeasuredQueryResult> {
  const startedAt = performance.now();
  const raw = await checker.getComponentMeta(component.absolutePath);
  const refined = refineMetaForBenchmark(raw);
  const propsJsonSchema = propsToJsonSchema(refined.props);
  const diagnostics = collectDiagnostics(raw, refined);
  const artifact = normalizeForBenchmark(
    component.relativePath,
    refined,
    propsJsonSchema,
    diagnostics,
  );
  const latencyMs = performance.now() - startedAt;
  const outcome: MetaUiOutcomeBucket = artifact.diagnostics.length > 0 ? "degraded" : "success";
  return {
    artifact,
    latencyMs,
    outcome,
  };
}

function collectDiagnostics(raw: any, refined: any) {
  const diagnostics = [];
  if (!raw) {
    diagnostics.push({
      level: "error" as const,
      code: "meta_ui_empty_meta",
      message: "Backend returned no metadata.",
    });
  }
  if (
    !Array.isArray(refined?.props) ||
    !Array.isArray(refined?.events) ||
    !Array.isArray(refined?.slots)
  ) {
    diagnostics.push({
      level: "warning" as const,
      code: "meta_ui_incomplete_surface",
      message: "Backend returned an incomplete metadata surface.",
    });
  }
  return diagnostics;
}
