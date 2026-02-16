import type { ViteCodegenResult } from "@verter/native";

export interface VueQuery {
  vue: boolean;
  type?: "script" | "template" | "style" | (string & {});
  index?: number;
  scoped?: boolean;
  lang?: string;
}

export interface ParsedVueRequest {
  filename: string;
  query: VueQuery;
}

export function parseVueRequest(id: string): ParsedVueRequest {
  const [filename, queryString] = id.split("?", 2);

  if (!queryString) {
    return { filename, query: { vue: false } };
  }

  const params = new URLSearchParams(queryString);

  const query: VueQuery = {
    vue: params.has("vue"),
    type: params.get("type") as VueQuery["type"],
    index: params.has("index") ? parseInt(params.get("index")!, 10) : undefined,
    scoped: params.has("scoped"),
    lang: params.get("lang") || undefined,
  };

  return { filename, query };
}

const cache = new Map<string, ViteCodegenResult>();

export function getDescriptor(filename: string): ViteCodegenResult | undefined {
  return cache.get(filename);
}

export function setDescriptor(filename: string, result: ViteCodegenResult): void {
  cache.set(filename, result);
}

export function deleteDescriptor(filename: string): void {
  cache.delete(filename);
}

export function clearCache(): void {
  cache.clear();
}
