/**
 * Editor-neutral behavioral contract for Verter's public LSP surface.
 *
 * The contract deliberately separates standard LSP behavior from Verter's
 * readiness/provider attestation and from provider-process topology. Editors can
 * reuse the standard cases through their own driver without pretending that a
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
  | "plain-control-hover"
  | "plain-control-definition"
  | "plain-control-completion"
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
  readonly expectedDefinitionDocument?: string;
  readonly expectedDefinitionAnchor?: ContractAnchor;
  readonly expectedDefinitionRange?: LspRange;
  readonly expectedRenameAnchors?: readonly ContractAnchor[];
  readonly requiredHoverFragments?: readonly string[];
  readonly forbiddenHoverPatterns?: readonly RegExp[];
  readonly expectedCompletion?: string;
  readonly expectedDiagnosticCode?: number;
  readonly renameTo?: string;
  readonly providers: readonly EditorNeutralProviderRoute[];
}

export interface EditorNeutralContractEvidence {
  /** First/repeated local script↔markup definition requests, bounded by the suite timeout. */
  readonly localDefinitionDurationsMs?: readonly [number, number];
}

/** A hard contract failure that still carries completed request-timing evidence. */
export class EditorNeutralContractFailure extends Error {
  readonly evidence: EditorNeutralContractEvidence;

  constructor(message: string, evidence: EditorNeutralContractEvidence) {
    super(message);
    this.name = "EditorNeutralContractFailure";
    this.evidence = evidence;
  }
}

interface CarrierContractSpec {
  readonly id: string;
  readonly framework: EditorNeutralFramework;
  readonly language: EditorNeutralScriptLanguage;
  readonly document: string;
  readonly errorDocument: string;
  readonly localAnchor: ContractAnchor;
  readonly localDeclarationAnchor: ContractAnchor;
  readonly completionAnchor: ContractAnchor;
  readonly expectedCompletion: string;
  readonly requiredLocalHover: readonly string[];
  readonly directConsumer: string;
  readonly directHoverAnchor: ContractAnchor;
  readonly directDefinitionAnchor: ContractAnchor;
  readonly barrelConsumer: string;
  readonly barrelHoverAnchor: ContractAnchor;
  readonly barrelDefinitionAnchor: ContractAnchor;
  readonly requiredPublicHover: readonly string[];
  readonly forbiddenPublicHover: readonly RegExp[];
}

interface DomEventContractSpec {
  readonly id: string;
  readonly framework: EditorNeutralFramework;
  readonly language: EditorNeutralScriptLanguage;
  readonly document: string;
  readonly errorDocument: string;
  readonly hoverAnchor: ContractAnchor;
}

interface LaxDomEventContractSpec {
  readonly id: string;
  readonly framework: EditorNeutralFramework;
  readonly document: string;
  readonly eventAnchor: ContractAnchor;
  readonly declarationAnchor: ContractAnchor;
  readonly completionAnchor: ContractAnchor;
}

const ALL_PROVIDERS = ["tsserver", "tsgo", "shared-tsgo"] as const;
const FILE_START_RANGE: LspRange = {
  start: { line: 0, character: 0 },
  end: { line: 0, character: 0 },
};

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
    localDeclarationAnchor: {
      text: "const vueTsLocal = 1;",
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
      text: "export const directVueTs = VueTypeScriptCase;",
      occurrence: 0,
      token: "VueTypeScriptCase",
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
    localDeclarationAnchor: {
      text: "const vueJsLocal = 1;",
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
      text: "export const directVueJs = VueJavaScriptCase;",
      occurrence: 0,
      token: "VueJavaScriptCase",
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
    localDeclarationAnchor: {
      text: "let svelteTsLocal = $state(1);",
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
      text: "export const directSvelteTs = SvelteTypeScriptCase;",
      occurrence: 0,
      token: "SvelteTypeScriptCase",
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
    localDeclarationAnchor: {
      text: "let svelteJsLocal = $state(1);",
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
      text: "export const directSvelteJs = SvelteJavaScriptCase;",
      occurrence: 0,
      token: "SvelteJavaScriptCase",
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
    requiredPublicHover: ["Component", "label", "focus"],
    forbiddenPublicHover: [
      /\bany\b/i,
      /\bunknown\b/i,
      /__VerterPublicInstance/,
      /new\s*\(\.\.\.args/,
    ],
  },
];

const DOM_EVENT_HANDLERS: readonly DomEventContractSpec[] = [
  {
    id: "vue-js-dom-event",
    framework: "vue",
    language: "js",
    document: "src/vue/JavaScriptEventHandler.vue",
    errorDocument: "src/vue/JavaScriptEventHandlerInvalid.vue",
    hoverAnchor: {
      text: "e.pointerId;",
      occurrence: 0,
      token: "e.pointerId",
    },
  },
  {
    id: "svelte-js-dom-event",
    framework: "svelte",
    language: "js",
    document: "src/svelte/JavaScriptEventHandler.svelte",
    errorDocument: "src/svelte/JavaScriptEventHandlerInvalid.svelte",
    hoverAnchor: {
      text: "e.pointerId;",
      occurrence: 0,
      token: "e.pointerId",
    },
  },
  {
    id: "vue-ts-dom-event",
    framework: "vue",
    language: "ts",
    document: "src/vue/TypeScriptEventHandler.vue",
    errorDocument: "src/vue/TypeScriptEventHandlerInvalid.vue",
    hoverAnchor: {
      text: "return e.pointerId;",
      occurrence: 0,
      token: "e.pointerId",
    },
  },
  {
    id: "svelte-ts-dom-event",
    framework: "svelte",
    language: "ts",
    document: "src/svelte/TypeScriptEventHandler.svelte",
    errorDocument: "src/svelte/TypeScriptEventHandlerInvalid.svelte",
    hoverAnchor: {
      text: "lastPointerId = e.pointerId;",
      occurrence: 0,
      token: "e.pointerId",
    },
  },
];

const LAX_DOM_EVENT_HANDLERS: readonly LaxDomEventContractSpec[] = [
  {
    id: "vue-js-dom-event-policy-lax",
    framework: "vue",
    document: "src/policy/lax/vue/JavaScriptEventHandler.vue",
    eventAnchor: {
      text: "e.pointerId;",
      occurrence: 0,
      token: "e.pointerId",
    },
    declarationAnchor: {
      text: "function myClick(e) {",
      occurrence: 0,
      token: "e",
    },
    completionAnchor: {
      text: "e.pointerId;",
      occurrence: 0,
      token: ".",
      offset: 1,
    },
  },
  {
    id: "svelte-js-dom-event-policy-lax",
    framework: "svelte",
    document: "src/policy/lax/svelte/JavaScriptEventHandler.svelte",
    eventAnchor: {
      text: "e.pointerId;",
      occurrence: 0,
      token: "e.pointerId",
    },
    declarationAnchor: {
      text: "function myClick(e) {",
      occurrence: 0,
      token: "e",
    },
    completionAnchor: {
      text: "e.pointerId;",
      occurrence: 0,
      token: ".",
      offset: 1,
    },
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
        expectedDefinitionDocument: carrier.document,
        expectedDefinitionAnchor: carrier.localDeclarationAnchor,
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
        expectedRenameAnchors: [carrier.localDeclarationAnchor, carrier.localAnchor],
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
        expectedDefinitionDocument: carrier.document,
        expectedDefinitionRange: FILE_START_RANGE,
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
        expectedDefinitionDocument: carrier.document,
        expectedDefinitionRange: FILE_START_RANGE,
      },
    );
  }

  for (const handler of DOM_EVENT_HANDLERS) {
    const base = {
      framework: handler.framework,
      language: handler.language,
      providers: ALL_PROVIDERS,
      surface: "standard-lsp" as const,
    };
    if (handler.language === "js") {
      cases.push(
        {
          ...base,
          id: `${handler.id}.diagnostics.non-inference-boundary`,
          feature: "diagnostics-clean",
          document: handler.document,
        },
        {
          ...base,
          id: `${handler.id}.jsdoc.hover`,
          feature: "hover",
          document: handler.errorDocument,
          anchor: handler.hoverAnchor,
          requiredHoverFragments: ["PointerEvent"],
          forbiddenHoverPatterns: [/\bany\b/i, /\bunknown\b/i],
        },
        {
          ...base,
          id: `${handler.id}.jsdoc.diagnostics.invalid-member-consumed`,
          feature: "diagnostics-clean",
          document: handler.errorDocument,
        },
      );
    } else {
      cases.push(
        {
          ...base,
          id: `${handler.id}.diagnostics.clean`,
          feature: "diagnostics-clean",
          document: handler.document,
        },
        {
          ...base,
          id: `${handler.id}.hover`,
          feature: "hover",
          document: handler.document,
          anchor: handler.hoverAnchor,
          requiredHoverFragments: ["PointerEvent"],
          forbiddenHoverPatterns: [/\bany\b/i, /\bunknown\b/i],
        },
        {
          ...base,
          id: `${handler.id}.diagnostics.invalid-member-consumed`,
          feature: "diagnostics-clean",
          document: handler.errorDocument,
        },
      );
    }
  }

  for (const handler of LAX_DOM_EVENT_HANDLERS) {
    const base = {
      framework: handler.framework,
      language: "js" as const,
      providers: ALL_PROVIDERS,
      surface: "standard-lsp" as const,
    };
    cases.push(
      {
        ...base,
        id: `${handler.id}.diagnostics.clean`,
        feature: "diagnostics-clean",
        document: handler.document,
      },
      {
        ...base,
        id: `${handler.id}.hover`,
        feature: "hover",
        document: handler.document,
        anchor: handler.eventAnchor,
        requiredHoverFragments: ["PointerEvent"],
        forbiddenHoverPatterns: [/\bany\b/i, /\bunknown\b/i],
      },
      {
        ...base,
        id: `${handler.id}.completion`,
        feature: "completion",
        document: handler.document,
        anchor: handler.completionAnchor,
        expectedCompletion: "pointerId",
      },
      {
        ...base,
        id: `${handler.id}.definition`,
        feature: "definition",
        document: handler.document,
        anchor: handler.eventAnchor,
        expectedDefinitionDocument: handler.document,
        expectedDefinitionAnchor: handler.declarationAnchor,
      },
    );
  }

  cases.push(
    {
      id: "svelte-classic-js-dom-event.diagnostics.non-inference-boundary",
      surface: "standard-lsp",
      feature: "diagnostics-clean",
      framework: "svelte",
      language: "js",
      document: "src/svelte/ClassicJavaScriptEventHandler.svelte",
      providers: ALL_PROVIDERS,
    },
    {
      id: "svelte-classic-ts-dom-event.diagnostics.non-inference-boundary",
      surface: "standard-lsp",
      feature: "diagnostics-clean",
      framework: "svelte",
      language: "ts",
      document: "src/svelte/ClassicTypeScriptEventHandler.svelte",
      providers: ALL_PROVIDERS,
    },
  );

  cases.push(
    {
      id: "svelte-ts-state-string.diagnostics.clean",
      surface: "standard-lsp",
      feature: "diagnostics-clean",
      framework: "svelte",
      language: "ts",
      document: "src/svelte/StateStringInterpolation.svelte",
      providers: ALL_PROVIDERS,
    },
    {
      id: "plain-jsx-pointer-event.hover",
      surface: "standard-lsp",
      feature: "plain-control-hover",
      document: "src/plain-pointer-control.jsx",
      anchor: {
        text: "e.pointerId;",
        occurrence: 0,
        token: "e.pointerId",
      },
      requiredHoverFragments: ["PointerEvent"],
      forbiddenHoverPatterns: [/\bany\b/i, /\bunknown\b/i],
      providers: ALL_PROVIDERS,
    },
    {
      id: "plain-jsx-pointer-event.diagnostics.invalid-member",
      surface: "standard-lsp",
      feature: "diagnostics-error",
      document: "src/plain-pointer-control.jsx",
      expectedDiagnosticCode: 2339,
      providers: ALL_PROVIDERS,
    },
    {
      id: "plain-tsx-pointer-event.hover",
      surface: "standard-lsp",
      feature: "plain-control-hover",
      document: "src/plain-pointer-control.tsx",
      anchor: {
        text: "e.pointerId;",
        occurrence: 0,
        token: "e.pointerId",
      },
      requiredHoverFragments: ["PointerEvent"],
      forbiddenHoverPatterns: [/\bany\b/i, /\bunknown\b/i, /\bEvent\b/],
      providers: ALL_PROVIDERS,
    },
    {
      id: "plain-tsx-pointer-event.diagnostics.invalid-member",
      surface: "standard-lsp",
      feature: "diagnostics-error",
      document: "src/plain-pointer-control.tsx",
      expectedDiagnosticCode: 2339,
      providers: ALL_PROVIDERS,
    },
    {
      id: "plain-typescript.hover",
      surface: "standard-lsp",
      feature: "plain-control-hover",
      document: "src/plain-control.ts",
      anchor: {
        text: "plainControlNumber.toFixed(0)",
        occurrence: 0,
        token: "plainControlNumber",
      },
      requiredHoverFragments: ["number"],
      forbiddenHoverPatterns: [/\bany\b/i, /\bunknown\b/i],
      providers: ALL_PROVIDERS,
    },
    {
      id: "plain-typescript.definition",
      surface: "standard-lsp",
      feature: "plain-control-definition",
      document: "src/plain-control.ts",
      anchor: {
        text: "plainControlNumber.toFixed(0)",
        occurrence: 0,
        token: "plainControlNumber",
      },
      expectedDefinitionDocument: "src/plain-control.ts",
      expectedDefinitionAnchor: {
        text: "export const plainControlNumber = 1;",
        occurrence: 0,
        token: "plainControlNumber",
      },
      providers: ALL_PROVIDERS,
    },
    {
      id: "plain-typescript.completion",
      surface: "standard-lsp",
      feature: "plain-control-completion",
      document: "src/plain-control.ts",
      anchor: {
        text: "plainControlNumber.toFixed(0)",
        occurrence: 0,
        token: ".",
        offset: 1,
      },
      expectedCompletion: "toFixed",
      providers: ALL_PROVIDERS,
    },
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
function contractAnchorUtf16Range(
  source: string,
  anchor: ContractAnchor,
): {
  readonly start: number;
  readonly end: number;
} {
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
  if (anchor.text.indexOf(anchor.token, tokenStart + anchor.token.length) >= 0) {
    throw new Error(
      `contract anchor token ${JSON.stringify(anchor.token)} is ambiguous in ${JSON.stringify(anchor.text)}`,
    );
  }
  const offset = anchor.offset ?? 0;
  if (!Number.isInteger(offset) || offset < 0 || offset > anchor.token.length) {
    throw new Error(`contract anchor offset must fall within its token`);
  }
  return {
    start: contextStart + tokenStart + offset,
    end: contextStart + tokenStart + anchor.token.length,
  };
}

export function resolveContractAnchor(
  source: string,
  anchor: ContractAnchor,
  encoding: PositionEncoding = "utf-16",
): LspPosition {
  const sourceOffset = contractAnchorUtf16Range(source, anchor).start;
  const byteOffset = Buffer.byteLength(source.slice(0, sourceOffset), "utf8");
  return new DocumentPositions(source).byteToPosition(byteOffset, encoding);
}

/** Resolve the exact source token range represented by a contextual anchor. */
export function resolveContractAnchorRange(
  source: string,
  anchor: ContractAnchor,
  encoding: PositionEncoding = "utf-16",
): LspRange {
  const offsets = contractAnchorUtf16Range(source, anchor);
  const positions = new DocumentPositions(source);
  return {
    start: positions.utf16ToPosition(offsets.start, encoding),
    end: positions.utf16ToPosition(offsets.end, encoding),
  };
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

function normalizedUriPath(uri: string): string {
  return decodeURIComponent(uri).replaceAll("\\", "/").toLowerCase();
}

function uriTargetsDocument(uri: string, document: string): boolean {
  return normalizedUriPath(uri).endsWith(`/${document.replaceAll("\\", "/").toLowerCase()}`);
}

function definitionSelectionRange(location: LspLocation): LspRange | undefined {
  return location.targetSelectionRange ?? location.range ?? location.targetRange;
}

function rangeKey(range: LspRange): string {
  return `${range.start.line}:${range.start.character}-${range.end.line}:${range.end.character}`;
}

function validateDefinitionResult(
  testCase: EditorNeutralContractCase,
  result: unknown,
  driver: EditorNeutralContractDriver,
  sources: ReadonlyMap<string, string>,
  attempt: string,
): void {
  const locations = definitionLocations(result);
  if (locations.length === 0) throw new Error(`${testCase.id}: ${attempt} definition was empty`);
  const uris = locations.map((location) => location.targetUri ?? location.uri ?? "");
  if (uris.some((uri) => /\.(?:vue|svelte)\.(?:tsx|jsx|ts|js)$/.test(normalizedUriPath(uri)))) {
    throw new Error(`${testCase.id}: generated carrier URI leaked: ${uris.join(", ")}`);
  }

  const targetDocument = testCase.expectedDefinitionDocument;
  if (!targetDocument) throw new Error(`${testCase.id}: contract has no definition document`);
  const targetLocations = locations.filter((location) =>
    uriTargetsDocument(location.targetUri ?? location.uri ?? "", targetDocument),
  );
  if (targetLocations.length === 0) {
    throw new Error(
      `${testCase.id}: ${attempt} definition did not reach /${targetDocument}; got ${uris.join(", ")}`,
    );
  }

  const hasAnchor = testCase.expectedDefinitionAnchor !== undefined;
  const hasRange = testCase.expectedDefinitionRange !== undefined;
  if (Number(hasAnchor) + Number(hasRange) !== 1) {
    throw new Error(`${testCase.id}: contract must own exactly one declaration range`);
  }
  const expectedRange = testCase.expectedDefinitionAnchor
    ? resolveContractAnchorRange(
        sources.get(targetDocument) ??
          (() => {
            throw new Error(`${testCase.id}: missing definition source for ${targetDocument}`);
          })(),
        testCase.expectedDefinitionAnchor,
        driver.positionEncoding ?? "utf-16",
      )
    : testCase.expectedDefinitionRange!;
  const actualRanges = targetLocations
    .map(definitionSelectionRange)
    .filter((range): range is LspRange => range !== undefined);
  if (!actualRanges.some((range) => rangeKey(range) === rangeKey(expectedRange))) {
    throw new Error(
      `${testCase.id}: ${attempt} definition reached the document at the wrong declaration range; expected ${rangeKey(expectedRange)}, got ${actualRanges.map(rangeKey).join(", ") || "no range"}`,
    );
  }
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
): Promise<void | EditorNeutralContractEvidence> {
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
    case "barrel-import-hover":
    case "plain-control-hover": {
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
    case "barrel-import-definition":
    case "plain-control-definition": {
      const requestCount = testCase.feature === "definition" ? 2 : 1;
      const durations: number[] = [];
      const failures: string[] = [];
      for (let index = 0; index < requestCount; index += 1) {
        const startedAt = performance.now();
        const result = await driver.definition(
          testCase.document,
          positionFor(testCase, driver, sources),
        );
        durations.push(Math.round(performance.now() - startedAt));
        try {
          validateDefinitionResult(
            testCase,
            result,
            driver,
            sources,
            requestCount === 2 ? (index === 0 ? "first" : "repeated") : "single",
          );
        } catch (error) {
          failures.push(error instanceof Error ? error.message : String(error));
        }
      }
      const evidence: EditorNeutralContractEvidence | undefined =
        requestCount === 2
          ? { localDefinitionDurationsMs: durations as [number, number] }
          : undefined;
      if (failures.length > 0) {
        if (evidence) throw new EditorNeutralContractFailure(failures.join(" | "), evidence);
        throw new Error(failures.join(" | "));
      }
      return evidence;
    }
    case "completion":
    case "plain-control-completion": {
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
      const expectedAnchors = testCase.expectedRenameAnchors ?? [];
      if (expectedAnchors.length === 0) {
        throw new Error(`${testCase.id}: contract has no exact rename anchors`);
      }
      if (edits.length !== expectedAnchors.length) {
        throw new Error(
          `${testCase.id}: rename produced ${edits.length} edits, expected exactly ${expectedAnchors.length}`,
        );
      }
      if (edits.some(({ uri }) => /\.(?:vue|svelte)\.(?:tsx|jsx|ts|js)$/i.test(uri))) {
        throw new Error(`${testCase.id}: rename leaked a generated carrier URI`);
      }
      if (edits.some(({ edit }) => edit.newText !== testCase.renameTo)) {
        throw new Error(`${testCase.id}: rename edit newText did not equal ${testCase.renameTo}`);
      }
      const sourceEdits = edits.filter(({ uri }) => uriTargetsDocument(uri, testCase.document));
      if (sourceEdits.length !== edits.length) {
        throw new Error(`${testCase.id}: rename did not map every edit to ${testCase.document}`);
      }

      const source = requireSource(testCase, sources);
      const encoding = driver.positionEncoding ?? "utf-16";
      const positions = new DocumentPositions(source);
      const originalToken = testCase.anchor?.token;
      if (!originalToken) throw new Error(`${testCase.id}: rename contract has no original token`);
      for (const { edit } of sourceEdits) {
        const start = positions.positionToUtf16(edit.range.start, encoding);
        const end = positions.positionToUtf16(edit.range.end, encoding);
        if (source.slice(start, end) !== originalToken) {
          throw new Error(
            `${testCase.id}: rename edit did not select the original token ${JSON.stringify(originalToken)}`,
          );
        }
      }

      const expectedRangeKeys = expectedAnchors
        .map((anchor) => rangeKey(resolveContractAnchorRange(source, anchor, encoding)))
        .sort();
      const actualRangeKeys = sourceEdits.map(({ edit }) => rangeKey(edit.range)).sort();
      if (JSON.stringify(actualRangeKeys) !== JSON.stringify(expectedRangeKeys)) {
        throw new Error(
          `${testCase.id}: rename ranges differ; expected ${expectedRangeKeys.join(", ")}, got ${actualRangeKeys.join(", ")}`,
        );
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
