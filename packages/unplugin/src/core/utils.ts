import { parseRequestId } from "@verter/native";
import type { HostParsedRequestId } from "@verter/native";

export interface VueQuery {
  vue: boolean;
  verter: boolean;
  type?: "script" | "template" | "style" | (string & {});
  index?: number;
  scoped?: boolean;
  lang?: string;
}

export interface ParsedVueRequest {
  filename: string;
  query: VueQuery;
}

/** The query fields of a host-parsed request, in the vocabulary of `VueQuery`. */
export function vueQueryOf(request: HostParsedRequestId): VueQuery {
  const { kind, index } = request.nodeKind;
  return {
    vue: request.marker === "vue",
    verter: request.marker === "verter",
    type: kind === "main" ? undefined : kind === "custom" ? request.blockType : kind,
    index,
    scoped: request.scoped,
    lang: request.lang,
  };
}

/**
 * Splits a request id into the file path and the query flags this package
 * reads. A projection of the host's id grammar: a refused request still states
 * its path and the marker it carried.
 */
export function parseVueRequest(id: string): ParsedVueRequest {
  const parsed = parseRequestId(id);
  if ("rejected" in parsed) {
    return {
      filename: parsed.rawPath,
      query: { vue: parsed.marker === "vue", verter: parsed.marker === "verter" },
    };
  }
  return { filename: parsed.rawPath, query: vueQueryOf(parsed) };
}
