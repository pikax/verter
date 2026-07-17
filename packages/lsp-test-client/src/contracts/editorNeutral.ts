/**
 * Editor-neutral behavioral contract for Verter's public LSP surface.
 *
 * The contract deliberately separates standard LSP behavior from Verter's
 * readiness/provider attestation and from provider-process topology. Editors can
 * reuse the 41 standard cases through their own driver without pretending that a
 * Verter custom notification is part of the Language Server Protocol.
 */
import { DocumentPositions, type LspPosition, type PositionEncoding } from "../positionEncoding.js";

export type EditorNeutralProviderRoute = "tsserver" | "tsgo" | "shared-tsgo";
export type EditorNeutralFramework = "vue" | "svelte";
export type EditorNeutralScriptLanguage = "ts" | "js";
export type EditorNeutralContractSurface =
  | "standard-lsp"
  | "verter-custom-protocol"
  | "provider-topology";

export type EditorNeutralContractFeature =
  | "diagnostics-clean"
  | "diagnostics-error"
  | "hover"
  | "definition"
  | "completion"
  | "rename"
  | "direct-import-hover"
  | "direct-import-definition"
  | "barrel-import-hover"
  | "barrel-import-definition"
  | "consumer-diagnostics"
  | "provider-attestation"
  | "shared-provider-topology";

export interface LspRange {
  readonly start: LspPosition;
  readonly end: LspPosition;
}

export interface LspDiagnostic {
  readonly range: LspRange;
  readonly message: string;
  readonly code?: string | number | { readonly value?: string | number };
  readonly severity?: number;
  readonly source?: string;
}

export interface LspLocation {
  readonly uri?: string;
  readonly range?: LspRange;
  readonly targetUri?: string;
  readonly targetRange?: LspRange;
  readonly targetSelectionRange?: LspRange;
}

export interface LspCompletionItem {
  readonly label: string | { readonly label: string };
}

export interface LspCompletionList {
  readonly items?: readonly LspCompletionItem[];
}

export interface LspTextEdit {
  readonly range: LspRange;
  readonly newText: string;
}

export interface LspTextDocumentEdit {
  readonly textDocument: { readonly uri: string };
  readonly edits: readonly LspTextEdit[];
}

export interface LspWorkspaceEdit {
  readonly changes?: Readonly<Record<string, readonly LspTextEdit[]>>;
  readonly documentChanges?: readonly (LspTextDocumentEdit | unknown)[];
}

export interface ProviderAttestation {
  readonly route: EditorNeutralProviderRoute;
  readonly publicKind: "tsserver" | "tsgo" | "editor-tsserver" | "none";
  readonly reason?: string;
  readonly startedKinds: readonly string[];
}

export interface ProviderTopologyAttestation {
  readonly managedFallbackStarted: boolean;
  readonly sharedRelayAlive: boolean;
  readonly detail?: string;
}

/** The narrow, editor-independent operations consumed by the shared contract. */
export interface EditorNeutralContractDriver {
  route: EditorNeutralProviderRoute;
  readonly positionEncoding?: PositionEncoding;
  diagnostics(document: string): Promise<readonly LspDiagnostic[]>;
  hover(document: string, position: LspPosition): Promise<unknown>;
  definition(document: string, position: LspPosition): Promise<unknown>;
  completion(document: string, position: LspPosition): Promise<unknown>;
  rename(
    document: string,
    position: LspPosition,
    newName: string,
  ): Promise<LspWorkspaceEdit | null>;
  attestProvider(): Promise<ProviderAttestation>;
  attestTopology(): Promise<ProviderTopologyAttestation>;
}

/**
 * An anchor is intentionally contextual. A bare token search is too easy to
 * redirect to a declaration when a fixture edit was meant to probe markup.
 */
export interface ContractAnchor {
  readonly text: string;
  readonly occurrence?: number;
  readonly token: string;
  /** UTF-16 source-unit offset inside `token`; completion commonly uses 1 after `.`. */
  readonly offset?: number;
}

export interface EditorNeutralContractCase {
  readonly id: string;
  readonly surface: EditorNeutralContractSurface;
  readonly feature: EditorNeutralContractFeature;
  readonly framework?: EditorNeutralFramework;
  readonly language?: EditorNeutralScriptLanguage;
  readonly document: string;
  readonly documents?: readonly string[];
  readonly anchor?: ContractAnchor;
  readonly expectedTargetSuffix?: string;
  readonly requiredHoverFragments?: readonly string[];
  readonly forbiddenHoverPatterns?: readonly RegExp[];
  readonly expectedCompletion?: string;
  readonly expectedDiagnosticCode?: number;
  readonly renameTo?: string;
  readonly providers: readonly EditorNeutralProviderRoute[];
}

interface CarrierContractSpec {
  readonly id: string;
  readonly framework: EditorNeutralFramework;
  readonly language: EditorNeutralScriptLanguage;
  readonly document: string;
  readonly errorDocument: string;
  readonly localAnchor: ContractAnchor;
  readonly completionAnchor: ContractAnchor;
  readonly expectedCompletion: string;
  readonly requiredLocalHover: readonly string[];
  readonly directConsumer: string;
  readonly directHoverAnchor: ContractAnchor;
  readonly directDefinitionAnchor: ContractAnchor;
  readonly barrelConsumer: string;
  readonly barrelHoverAnchor: ContractAnchor;
  readonly barrelDefinitionAnchor: ContractAnchor;
  readonly expectedTargetSuffix: string;
  readonly requiredPublicHover: readonly string[];
  readonly forbiddenPublicHover: readonly RegExp[];
}

const ALL_PROVIDERS = ["tsserver", "tsgo", "shared-tsgo"] as const;

const CARRIERS: readonly CarrierContractSpec[] = [
  {
    id: "vue-ts",
    framework: "vue",
    language: "ts",
    document: "src/vue/TypeScriptCase.vue",
    errorDocument: "src/diagnostics/InvalidTypeScript.vue",
    localAnchor: {
      text: "{{ vueTsLocal.toFixed(0) }}",
      occurrence: 0,
      token: "vueTsLocal",
    },
    completionAnchor: {
      text: "{{ vueTsLocal.toFixed(0) }}",
      occurrence: 0,
      token: ".",
      offset: 1,
    },
    expectedCompletion: "toFixed",
    requiredLocalHover: ["number"],
    directConsumer: "src/direct-consumer.ts",
    directHoverAnchor: {
      text: "export const directVueTs = VueTypeScriptCase;",
      occurrence: 0,
      token: "VueTypeScriptCase",
    },
    directDefinitionAnchor: {
      text: 'import VueTypeScriptCase from "./vue/TypeScriptCase.vue";',
      occurrence: 0,
      token: "./vue/TypeScriptCase.vue",
      offset: 3,
    },
    barrelConsumer: "src/barrel-consumer.ts",
    barrelHoverAnchor: {
      text: "export const barrelVueTs = VueTypeScriptCase;",
      occurrence: 0,
      token: "VueTypeScriptCase",
    },
    barrelDefinitionAnchor: {
      text: "export const barrelVueTs = VueTypeScriptCase;",
      occurrence: 0,
      token: "VueTypeScriptCase",
    },
    expectedTargetSuffix: "/vue/TypeScriptCase.vue",
    requiredPublicHover: ["label"],
    forbiddenPublicHover: [/\bany\b/i, /\bunknown\b/i, /__Verter\w*/],
  },
  {
    id: "vue-js",
    framework: "vue",
    language: "js",
    document: "src/vue/JavaScriptCase.vue",
    errorDocument: "src/diagnostics/InvalidJavaScript.vue",
    localAnchor: {
      text: "{{ vueJsLocal.toFixed(0) }}",
      occurrence: 0,
      token: "vueJsLocal",
    },
    completionAnchor: {
      text: "{{ vueJsLocal.toFixed(0) }}",
      occurrence: 0,
      token: ".",
      offset: 1,
    },
    expectedCompletion: "toFixed",
    requiredLocalHover: ["number"],
    directConsumer: "src/direct-consumer.ts",
    directHoverAnchor: {
      text: "export const directVueJs = VueJavaScriptCase;",
      occurrence: 0,
      token: "VueJavaScriptCase",
    },
    directDefinitionAnchor: {
      text: 'import VueJavaScriptCase from "./vue/JavaScriptCase.vue";',
      occurrence: 0,
      token: "./vue/JavaScriptCase.vue",
      offset: 3,
    },
    barrelConsumer: "src/barrel-consumer.ts",
    barrelHoverAnchor: {
      text: "export const barrelVueJs = VueJavaScriptCase;",
      occurrence: 0,
      token: "VueJavaScriptCase",
    },
    barrelDefinitionAnchor: {
      text: "export const barrelVueJs = VueJavaScriptCase;",
      occurrence: 0,
      token: "VueJavaScriptCase",
    },
    expectedTargetSuffix: "/vue/JavaScriptCase.vue",
    requiredPublicHover: ["label"],
    forbiddenPublicHover: [/\bany\b/i, /\bunknown\b/i, /__Verter\w*/],
  },
  {
    id: "svelte-ts",
    framework: "svelte",
    language: "ts",
    document: "src/svelte/TypeScriptCase.svelte",
    errorDocument: "src/diagnostics/InvalidTypeScript.svelte",
    localAnchor: {
      text: "{svelteTsLocal.toFixed(0)}",
      occurrence: 0,
      token: "svelteTsLocal",
    },
    completionAnchor: {
      text: "{svelteTsLocal.toFixed(0)}",
      occurrence: 0,
      token: ".",
      offset: 1,
    },
    expectedCompletion: "toFixed",
    requiredLocalHover: ["number"],
    directConsumer: "src/direct-consumer.ts",
    directHoverAnchor: {
      text: "export const directSvelteTs = SvelteTypeScriptCase;",
      occurrence: 0,
      token: "SvelteTypeScriptCase",
    },
    directDefinitionAnchor: {
      text: 'import SvelteTypeScriptCase from "./svelte/TypeScriptCase.svelte";',
      occurrence: 0,
      token: "./svelte/TypeScriptCase.svelte",
      offset: 3,
    },
    barrelConsumer: "src/barrel-consumer.ts",
    barrelHoverAnchor: {
      text: "export const barrelSvelteTs = SvelteTypeScriptCase;",
      occurrence: 0,
      token: "SvelteTypeScriptCase",
    },
    barrelDefinitionAnchor: {
      text: "export const barrelSvelteTs = SvelteTypeScriptCase;",
      occurrence: 0,
      token: "SvelteTypeScriptCase",
    },
    expectedTargetSuffix: "/svelte/TypeScriptCase.svelte",
    requiredPublicHover: ["Component", "label", "focus"],
    forbiddenPublicHover: [
      /\bany\b/i,
      /\bunknown\b/i,
      /__VerterPublicInstance/,
      /new\s*\(\.\.\.args/,
    ],
  },
  {
    id: "svelte-js",
    framework: "svelte",
    language: "js",
    document: "src/svelte/JavaScriptCase.svelte",
    errorDocument: "src/diagnostics/InvalidJavaScript.svelte",
    localAnchor: {
      text: "{svelteJsLocal.toFixed(0)}",
      occurrence: 0,
      token: "svelteJsLocal",
    },
    completionAnchor: {
      text: "{svelteJsLocal.toFixed(0)}",
      occurrence: 0,
      token: ".",
      offset: 1,
    },
    expectedCompletion: "toFixed",
    requiredLocalHover: ["number"],
    directConsumer: "src/direct-consumer.ts",
    directHoverAnchor: {
      text: "export const directSvelteJs = SvelteJavaScriptCase;",
      occurrence: 0,
      token: "SvelteJavaScriptCase",
    },
    directDefinitionAnchor: {
      text: 'import SvelteJavaScriptCase from "./svelte/JavaScriptCase.svelte";',
      occurrence: 0,
      token: "./svelte/JavaScriptCase.svelte",
      offset: 3,
    },
    barrelConsumer: "src/barrel-consumer.ts",
    barrelHoverAnchor: {
      text: "export const barrelSvelteJs = SvelteJavaScriptCase;",
      occurrence: 0,
      token: "SvelteJavaScriptCase",
    },
    barrelDefinitionAnchor: {
      text: "export const barrelSvelteJs = SvelteJavaScriptCase;",
      occurrence: 0,
      token: "SvelteJavaScriptCase",
    },
    expectedTargetSuffix: "/svelte/JavaScriptCase.svelte",
    requiredPublicHover: ["Component", "label", "focus"],
    forbiddenPublicHover: [
      /\bany\b/i,
      /\bunknown\b/i,
      /__VerterPublicInstance/,
      /new\s*\(\.\.\.args/,
    ],
  },
];

/** Construct the exact contract inventory. The returned list is stable and deduplicated. */
export function createEditorNeutralContractInventory(): readonly EditorNeutralContractCase[] {
  const cases: EditorNeutralContractCase[] = [];
  for (const carrier of CARRIERS) {
    const base = {
      framework: carrier.framework,
      language: carrier.language,
      providers: ALL_PROVIDERS,
      surface: "standard-lsp" as const,
    };
    cases.push(
      {
        ...base,
        id: `${carrier.id}.diagnostics.clean`,
        feature: "diagnostics-clean",
        document: carrier.document,
      },
      {
        ...base,
        id: `${carrier.id}.diagnostics.error`,
        feature: "diagnostics-error",
        document: carrier.errorDocument,
        expectedDiagnosticCode: 2322,
      },
      {
        ...base,
        id: `${carrier.id}.markup.hover`,
        feature: "hover",
        document: carrier.document,
        anchor: carrier.localAnchor,
        requiredHoverFragments: carrier.requiredLocalHover,
        forbiddenHoverPatterns: [/\bany\b/i, /\bunknown\b/i],
      },
      {
        ...base,
        id: `${carrier.id}.markup.definition`,
        feature: "definition",
        document: carrier.document,
        anchor: carrier.localAnchor,
        expectedTargetSuffix: `/${carrier.document.replaceAll("\\", "/")}`,
      },
      {
        ...base,
        id: `${carrier.id}.markup.completion`,
        feature: "completion",
        document: carrier.document,
        anchor: carrier.completionAnchor,
        expectedCompletion: carrier.expectedCompletion,
      },
      {
        ...base,
        id: `${carrier.id}.markup.rename`,
        feature: "rename",
        document: carrier.document,
        anchor: carrier.localAnchor,
        renameTo: `${carrier.id.replace("-", "_")}_renamed`,
      },
      {
        ...base,
        id: `${carrier.id}.direct-import.hover`,
        feature: "direct-import-hover",
        document: carrier.directConsumer,
        anchor: carrier.directHoverAnchor,
        requiredHoverFragments: carrier.requiredPublicHover,
        forbiddenHoverPatterns: carrier.forbiddenPublicHover,
      },
      {
        ...base,
        id: `${carrier.id}.direct-import.definition`,
        feature: "direct-import-definition",
        document: carrier.directConsumer,
        anchor: carrier.directDefinitionAnchor,
        expectedTargetSuffix: carrier.expectedTargetSuffix,
      },
      {
        ...base,
        id: `${carrier.id}.barrel-import.hover`,
        feature: "barrel-import-hover",
        document: carrier.barrelConsumer,
        anchor: carrier.barrelHoverAnchor,
        requiredHoverFragments: carrier.requiredPublicHover,
        forbiddenHoverPatterns: carrier.forbiddenPublicHover,
      },
      {
        ...base,
        id: `${carrier.id}.barrel-import.definition`,
        feature: "barrel-import-definition",
        document: carrier.barrelConsumer,
        anchor: carrier.barrelDefinitionAnchor,
        expectedTargetSuffix: carrier.expectedTargetSuffix,
      },
    );
  }

  cases.push(
    {
      id: "consumer-imports.diagnostics",
      surface: "standard-lsp",
      feature: "consumer-diagnostics",
      document: "src/direct-consumer.ts",
      documents: ["src/direct-consumer.ts", "src/barrel-consumer.ts"],
      providers: ALL_PROVIDERS,
    },
    {
      id: "verter.provider-attestation",
      surface: "verter-custom-protocol",
      feature: "provider-attestation",
      document: "",
      providers: ALL_PROVIDERS,
    },
    {
      id: "shared-tsgo.real-relay-no-fallback",
      surface: "provider-topology",
      feature: "shared-provider-topology",
      document: "",
      providers: ["shared-tsgo"],
    },
  );

  return cases;
}

/** Resolve a contextual source anchor in the driver's negotiated position encoding. */
export function resolveContractAnchor(
  source: string,
  anchor: ContractAnchor,
  encoding: PositionEncoding = "utf-16",
): LspPosition {
  if (!Number.isInteger(anchor.occurrence) || (anchor.occurrence ?? -1) < 0) {
    throw new Error(`contract anchor occurrence must be an explicit non-negative integer`);
  }
  let contextStart = -1;
  let from = 0;
  for (let current = 0; current <= anchor.occurrence!; current += 1) {
    contextStart = source.indexOf(anchor.text, from);
    if (contextStart < 0) {
      throw new Error(
        `contract anchor text ${JSON.stringify(anchor.text)} occurrence ${anchor.occurrence} not found`,
      );
    }
    from = contextStart + anchor.text.length;
  }
  const tokenStart = anchor.text.indexOf(anchor.token);
  if (tokenStart < 0) {
    throw new Error(
      `contract anchor token ${JSON.stringify(anchor.token)} is absent from ${JSON.stringify(anchor.text)}`,
    );
  }
  const sourceOffset = contextStart + tokenStart + (anchor.offset ?? 0);
  const byteOffset = Buffer.byteLength(source.slice(0, sourceOffset), "utf8");
  return new DocumentPositions(source).byteToPosition(byteOffset, encoding);
}

function diagnosticCode(diagnostic: LspDiagnostic): string | undefined {
  const code = diagnostic.code;
  if (typeof code === "string" || typeof code === "number") return String(code);
  if (code && (typeof code.value === "string" || typeof code.value === "number")) {
    return String(code.value);
  }
  return undefined;
}

function describeDiagnostics(diagnostics: readonly LspDiagnostic[]): string {
  return diagnostics
    .map((diagnostic) => `${diagnosticCode(diagnostic) ?? "?"}: ${diagnostic.message}`)
    .join(" | ");
}

function assertNoInfrastructureDiagnostics(
  testCase: EditorNeutralContractCase,
  diagnostics: readonly LspDiagnostic[],
): void {
  for (const forbidden of ["7026", "2304", "2307"]) {
    if (diagnostics.some((diagnostic) => diagnosticCode(diagnostic) === forbidden)) {
      throw new Error(
        `${testCase.id}: forbidden TS${forbidden} diagnostic: ${describeDiagnostics(diagnostics)}`,
      );
    }
  }
}

function hoverText(result: unknown): string {
  if (!result || typeof result !== "object") return "";
  const contents = (result as { contents?: unknown }).contents;
  if (typeof contents === "string") return contents;
  if (Array.isArray(contents)) {
    return contents
      .map((entry) =>
        typeof entry === "string"
          ? entry
          : String((entry as { value?: unknown } | null)?.value ?? ""),
      )
      .join("\n");
  }
  if (contents && typeof contents === "object" && "value" in contents) {
    return String((contents as { value?: unknown }).value ?? "");
  }
  return "";
}

function definitionLocations(result: unknown): readonly LspLocation[] {
  if (!result) return [];
  return (Array.isArray(result) ? result : [result]).filter(
    (entry): entry is LspLocation => entry !== null && typeof entry === "object",
  );
}

function completionLabels(result: unknown): readonly string[] {
  if (!result) return [];
  const items = Array.isArray(result) ? result : ((result as LspCompletionList).items ?? []);
  return items
    .map((item) => {
      const label = (item as LspCompletionItem).label;
      return typeof label === "string" ? label : label?.label;
    })
    .filter((label): label is string => typeof label === "string" && label.length > 0);
}

interface NormalizedEdit {
  readonly uri: string;
  readonly edit: LspTextEdit;
}

function workspaceEdits(edit: LspWorkspaceEdit | null): readonly NormalizedEdit[] {
  if (!edit) return [];
  const out: NormalizedEdit[] = [];
  for (const [uri, edits] of Object.entries(edit.changes ?? {})) {
    for (const item of edits) out.push({ uri, edit: item });
  }
  for (const change of edit.documentChanges ?? []) {
    if (
      !change ||
      typeof change !== "object" ||
      !("textDocument" in change) ||
      !("edits" in change)
    ) {
      continue;
    }
    const documentChange = change as LspTextDocumentEdit;
    for (const item of documentChange.edits) {
      out.push({ uri: documentChange.textDocument.uri, edit: item });
    }
  }
  return out;
}

function requireSource(
  testCase: EditorNeutralContractCase,
  sources: ReadonlyMap<string, string>,
): string {
  const source = sources.get(testCase.document);
  if (source === undefined) {
    throw new Error(`${testCase.id}: missing source for ${testCase.document}`);
  }
  return source;
}

function positionFor(
  testCase: EditorNeutralContractCase,
  driver: EditorNeutralContractDriver,
  sources: ReadonlyMap<string, string>,
): LspPosition {
  if (!testCase.anchor) throw new Error(`${testCase.id}: contract case has no anchor`);
  return resolveContractAnchor(
    requireSource(testCase, sources),
    testCase.anchor,
    driver.positionEncoding ?? "utf-16",
  );
}

/** Execute one behavioral case. Every missing/empty prerequisite is a failure. */
export async function executeEditorNeutralContractCase(
  testCase: EditorNeutralContractCase,
  driver: EditorNeutralContractDriver,
  sources: ReadonlyMap<string, string>,
): Promise<void> {
  if (!testCase.providers.includes(driver.route)) {
    throw new Error(
      `${testCase.id}: route ${driver.route} is not applicable; filtering is required`,
    );
  }

  switch (testCase.feature) {
    case "diagnostics-clean": {
      const diagnostics = await driver.diagnostics(testCase.document);
      assertNoInfrastructureDiagnostics(testCase, diagnostics);
      const errors = diagnostics.filter((diagnostic) => diagnostic.severity === 1);
      if (errors.length > 0) {
        throw new Error(`${testCase.id}: expected zero errors, got ${describeDiagnostics(errors)}`);
      }
      return;
    }
    case "diagnostics-error": {
      const diagnostics = await driver.diagnostics(testCase.document);
      assertNoInfrastructureDiagnostics(testCase, diagnostics);
      const expected = String(testCase.expectedDiagnosticCode);
      const errors = diagnostics.filter((diagnostic) => diagnostic.severity === 1);
      if (errors.length !== 1 || diagnosticCode(errors[0]) !== expected) {
        throw new Error(
          `${testCase.id}: expected exactly one TS${expected} error, got ${describeDiagnostics(errors)}`,
        );
      }
      return;
    }
    case "consumer-diagnostics": {
      const documents = testCase.documents ?? [testCase.document];
      if (documents.length === 0) throw new Error(`${testCase.id}: no consumer documents`);
      for (const document of documents) {
        const diagnostics = await driver.diagnostics(document);
        assertNoInfrastructureDiagnostics(testCase, diagnostics);
        const errors = diagnostics.filter((diagnostic) => diagnostic.severity === 1);
        if (errors.length > 0) {
          throw new Error(
            `${testCase.id}: ${document} has import/type errors: ${describeDiagnostics(errors)}`,
          );
        }
      }
      return;
    }
    case "hover":
    case "direct-import-hover":
    case "barrel-import-hover": {
      const result = await driver.hover(testCase.document, positionFor(testCase, driver, sources));
      const text = hoverText(result);
      if (text.trim().length === 0) throw new Error(`${testCase.id}: hover was empty`);
      for (const fragment of testCase.requiredHoverFragments ?? []) {
        if (!text.includes(fragment)) {
          throw new Error(`${testCase.id}: hover is missing ${JSON.stringify(fragment)}: ${text}`);
        }
      }
      for (const pattern of testCase.forbiddenHoverPatterns ?? []) {
        if (pattern.test(text)) {
          throw new Error(`${testCase.id}: hover matched forbidden ${pattern}: ${text}`);
        }
      }
      return;
    }
    case "definition":
    case "direct-import-definition":
    case "barrel-import-definition": {
      const result = await driver.definition(
        testCase.document,
        positionFor(testCase, driver, sources),
      );
      const locations = definitionLocations(result);
      if (locations.length === 0) throw new Error(`${testCase.id}: definition was empty`);
      const suffix = testCase.expectedTargetSuffix!.replaceAll("\\", "/").toLowerCase();
      const uris = locations
        .map((location) => location.targetUri ?? location.uri ?? "")
        .map((uri) => decodeURIComponent(uri).replaceAll("\\", "/").toLowerCase());
      if (!uris.some((uri) => uri.endsWith(suffix))) {
        throw new Error(
          `${testCase.id}: definition did not reach ${suffix}; got ${uris.join(", ")}`,
        );
      }
      if (uris.some((uri) => /\.(?:vue|svelte)\.(?:tsx|jsx|ts|js)$/.test(uri))) {
        throw new Error(`${testCase.id}: generated carrier URI leaked: ${uris.join(", ")}`);
      }
      return;
    }
    case "completion": {
      const result = await driver.completion(
        testCase.document,
        positionFor(testCase, driver, sources),
      );
      const labels = completionLabels(result);
      if (labels.length === 0) throw new Error(`${testCase.id}: completion returned zero items`);
      if (!labels.includes(testCase.expectedCompletion!)) {
        throw new Error(
          `${testCase.id}: completion lacks ${testCase.expectedCompletion}; got ${labels.join(", ")}`,
        );
      }
      return;
    }
    case "rename": {
      const result = await driver.rename(
        testCase.document,
        positionFor(testCase, driver, sources),
        testCase.renameTo!,
      );
      const edits = workspaceEdits(result);
      if (edits.length < 2) {
        throw new Error(
          `${testCase.id}: rename produced ${edits.length} edits, expected script + markup`,
        );
      }
      if (edits.some(({ uri }) => /\.(?:vue|svelte)\.(?:tsx|jsx|ts|js)$/i.test(uri))) {
        throw new Error(`${testCase.id}: rename leaked a generated carrier URI`);
      }
      const sourceEdits = edits.filter(({ uri }) =>
        decodeURIComponent(uri)
          .replaceAll("\\", "/")
          .toLowerCase()
          .endsWith(`/${testCase.document.replaceAll("\\", "/").toLowerCase()}`),
      );
      if (sourceEdits.length < 2) {
        throw new Error(`${testCase.id}: rename did not map both edits to ${testCase.document}`);
      }
      const distinctLines = new Set(sourceEdits.map(({ edit }) => edit.range.start.line));
      if (distinctLines.size < 2) {
        throw new Error(`${testCase.id}: rename did not span script and markup lines`);
      }
      return;
    }
    case "provider-attestation": {
      const attestation = await driver.attestProvider();
      if (attestation.route !== driver.route) {
        throw new Error(`${testCase.id}: attested ${attestation.route}, expected ${driver.route}`);
      }
      if (driver.route === "tsserver") {
        if (
          !["tsserver", "editor-tsserver"].includes(attestation.publicKind) ||
          !attestation.startedKinds.includes("tsserver")
        ) {
          throw new Error(`${testCase.id}: tsserver provider was not started/attested`);
        }
      } else if (driver.route === "tsgo") {
        if (attestation.publicKind !== "tsgo" || !attestation.startedKinds.includes("tsgo")) {
          throw new Error(`${testCase.id}: managed tsgo provider was not started/attested`);
        }
      } else if (
        attestation.publicKind !== "tsgo" ||
        !/editor-owned Native Preview/i.test(attestation.reason ?? "") ||
        attestation.startedKinds.length !== 0
      ) {
        throw new Error(
          `${testCase.id}: shared route lacks editor-owned provenance or already started fallback: ${JSON.stringify(attestation)}`,
        );
      }
      return;
    }
    case "shared-provider-topology": {
      if (driver.route !== "shared-tsgo") {
        throw new Error(`${testCase.id}: topology case requires shared-tsgo`);
      }
      const attestation = await driver.attestTopology();
      if (!attestation.sharedRelayAlive) {
        throw new Error(
          `${testCase.id}: real shared relay is not alive (${attestation.detail ?? ""})`,
        );
      }
      if (attestation.managedFallbackStarted) {
        throw new Error(
          `${testCase.id}: managed fallback was activated (${attestation.detail ?? ""})`,
        );
      }
      return;
    }
  }
}
