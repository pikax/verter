import {
  CANONICAL_TS,
  CANONICAL_VUE,
  TYPEINFO_SYMBOL,
  normalizeSymbolIdentities,
  canonicalizeTypeInfo,
} from "./index.js";

const VUE_COMPILE_REQUEST = {
  vue: {
    identity: { isProduction: false, forceJs: false },
    products: [
      { runtimeClient: { runtimeSourceMap: true } },
      { analysis: { wantScriptBindings: false, wantTemplateData: true } },
    ],
    options: {
      backend: "inferred",
      ssr: false,
      isCustomElement: [],
      babelParserPlugins: [],
      scriptCustomElement: false,
    },
  },
};

function parseMaybeJson(value) {
  if (typeof value !== "string") return value;
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function mappingFromCompile(response) {
  const maps = [];
  const products = response?.products;
  if (!Array.isArray(products)) return maps;
  for (const product of products) {
    if (Array.isArray(product.nodes)) {
      for (const node of product.nodes) {
        if (typeof node.sourceMap === "string" && node.sourceMap.length > 0)
          maps.push(node.sourceMap);
      }
    }
    if (typeof product.sourceMap === "string" && product.sourceMap.length > 0) {
      maps.push(product.sourceMap);
    }
  }
  return maps;
}

/**
 * Drive the existing VerterHost (WASM binding or native-equivalent shape).
 * Does not implement semantics; it only calls the session authority.
 */
export function runProbes(host, { tsSource, vueSource }) {
  host.upsert({
    canonicalId: CANONICAL_TS,
    inputId: CANONICAL_TS,
    source: tsSource,
    fileKind: "non_sfc",
  });
  host.upsert({
    canonicalId: CANONICAL_VUE,
    inputId: CANONICAL_VUE,
    source: vueSource,
    fileKind: "vue",
  });

  const compile = host.compileRequest(CANONICAL_VUE, VUE_COMPILE_REQUEST);
  const products = Array.isArray(compile?.products) ? compile.products : [];

  const rawSymbols = parseMaybeJson(host.listSymbols(CANONICAL_TS));
  const symbols = Array.isArray(rawSymbols) ? rawSymbols : [];
  const typeinfo = host.resolveSymbolWithAudit(CANONICAL_TS, TYPEINFO_SYMBOL, null, null);
  const typeExpr = canonicalizeTypeInfo(typeinfo?.typeExpr ?? typeinfo?.type_expr ?? null);

  const style = host.matchCssSelectors(CANONICAL_VUE);
  const mapping = mappingFromCompile(compile);
  const query = parseMaybeJson(host.resolveTypeWithAudit(CANONICAL_TS, TYPEINFO_SYMBOL));
  // The audit-enabled query must derive a stored record; a null result, an
  // error-shaped object or `hasRecord: false` is not a successful operation.
  if (query == null || typeof query !== "object" || query.hasRecord !== true) {
    throw new Error("query audit record is absent from resolveTypeWithAudit");
  }

  return {
    operations: {
      session: products,
      typeinfo: symbols,
      style: Array.isArray(style) ? style : [],
      mapping,
      query: [query],
    },
    identities: {
      symbols: normalizeSymbolIdentities(symbols),
      typeinfo: typeExpr,
    },
  };
}
