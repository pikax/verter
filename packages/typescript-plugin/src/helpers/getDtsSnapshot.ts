import type tsModule from "typescript/lib/tsserverlibrary";
import type { VerterHost } from "@verter/native";

export const FALLBACK_STUB = "export default {} as any";

let host: VerterHost | null = null;
let loadFailed = false;

function getHost(): VerterHost | null {
  if (host) return host;
  if (loadFailed) return null;

  try {
    const native: typeof import("@verter/native") = require("@verter/native");
    host = new native.VerterHost();
    return host;
  } catch {
    loadFailed = true;
    return null;
  }
}

export const parseFile = (
  fileName: string,
  content: string,
  logger: tsModule.server.Logger,
): string => {
  logger.info(`[Verter] parsing ${fileName}`);

  const h = getHost();
  if (!h) {
    logger.info("[Verter] native binary not available, returning stub");
    return FALLBACK_STUB;
  }

  try {
    h.upsert({ inputId: fileName, source: content });

    // getTsc() performs macro-only extraction (fast path — no full template compilation).
    // The generated code includes a //# sourceMappingURL= for Go-to-Definition support.
    const tsc = h.getTsc(fileName);
    if (!tsc) {
      logger.info(`[Verter] getTsc returned null for ${fileName}, no script block`);
      return FALLBACK_STUB;
    }

    logger.info(`[Verter] compiled ${fileName} (${tsc.code.length} chars)`);
    return tsc.code;
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    logger.info(`[Verter] compilation error for ${fileName}: ${msg}`);
    return FALLBACK_STUB;
  }
};
