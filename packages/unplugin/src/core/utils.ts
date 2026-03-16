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

  // Parse lang from both `lang=less` and `lang.less` (Vite dot-notation) formats
  let lang = params.get("lang") || undefined;
  if (!lang) {
    for (const key of params.keys()) {
      if (key.startsWith("lang.")) {
        lang = key.slice(5);
        break;
      }
    }
  }

  const query: VueQuery = {
    vue: params.has("vue"),
    type: params.get("type") as VueQuery["type"],
    index: params.has("index") ? parseInt(params.get("index")!, 10) : undefined,
    scoped: params.has("scoped"),
    lang,
  };

  return { filename, query };
}
