import {
  CAPTURE_STATES,
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
 *
 * `capture` is the module's `captureAvailability()`: `"Available"` when it
 * was built with `semantic-observe` (the query must return its stored audit
 * record), `"Unavailable"` otherwise (the query must return no record).
 */
export function runProbes(host, { tsSource, vueSource, capture }) {
  if (!CAPTURE_STATES.includes(capture)) {
    throw new Error(`capture availability is not reported (got ${String(capture)})`);
  }
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
  // `resolveTypeWithAudit` hands back the stored `RequestAuditRecord` (JSON)
  // only when the audit capture state is `ActiveStored`, and `null` when the
  // record was filtered, audit is disabled, or capture is unavailable.
  // `hasRecord` is derived from that state exactly like the native probe
  // derives it. With capture available anything else (null, an error-shaped
  // object, an array) is not a successful query operation; with capture
  // unavailable any returned record contradicts the reported availability.
  const record = parseMaybeJson(host.resolveTypeWithAudit(CANONICAL_TS, TYPEINFO_SYMBOL));
  const hasRecord =
    record != null &&
    typeof record === "object" &&
    !Array.isArray(record) &&
    record.capture_state === "ActiveStored";
  if (capture === "Available" && !hasRecord) {
    throw new Error("query audit record is absent from resolveTypeWithAudit");
  }
  if (capture === "Unavailable" && record != null) {
    throw new Error("resolveTypeWithAudit returned a record although capture is unavailable");
  }

  return {
    operations: {
      session: products,
      typeinfo: symbols,
      style: Array.isArray(style) ? style : [],
      mapping,
      query: [{ decl: TYPEINFO_SYMBOL, capture, kind: hasRecord ? record.kind : null, hasRecord }],
    },
    identities: {
      symbols: normalizeSymbolIdentities(symbols),
      typeinfo: typeExpr,
    },
  };
}
