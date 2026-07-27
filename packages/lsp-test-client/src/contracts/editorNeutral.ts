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
  | "shared-provider-topology"
  /**
   * Fail-closed CSS class boundary: a markup class token with NO declaring
   * rule must produce an EMPTY hover and an EMPTY definition — never a
   * mis-mapped affordance (e.g. a same-named script binding).
   */
  | "css-class-silent"
  /**
   * The DEFAULT-configuration counterpart of an opt-in Verter-native feature:
   * with no initialization options, the feature must answer NOTHING. Pins the
   * shipped default so a silent flip in either direction is a test failure.
   */
  | "verter-native-default-off";

/**
 * The server configuration a case is executed against.
 *
 * Most of the contract is configuration-independent and runs on `default` — a
 * bare `initialize` with no `initializationOptions`, exactly what an editor that
 * wires nothing sends.
 *
 * `verter-native-semantics` is the DOCUMENTED opt-in lane. Verter's native
 * markup/CSS intelligence is deliberately off by default (see
 * `EDITOR_NEUTRAL_SERVER_PROFILES`), so a case asserting those features has to
 * request them; asserting them on `default` would be asserting a capability the
 * product does not claim to offer there.
 */
export type EditorNeutralServerProfile = "default" | "verter-native-semantics";

/**
 * `initializationOptions` for each profile — the single authority shared by the
 * contract inventory and every driver, so a case's declared profile and the
 * server it actually talks to cannot drift.
 *
 * `hover.nativeSemantics` gates the native hover lane
 * (`crates/verter_lsp/src/config.rs::parse_hover_init_options`) and
 * `analysis.enabled` gates the semantic-enrichment snapshot that carries the
 * TEMPLATE half of the analysis (`parse_native_semantic_options`). CSS class
 * intelligence needs BOTH: without the first the native lane is never consulted,
 * and without the second `FileAnalysisSnapshot.template` is `None`, so every
 * markup-side native feature has nothing to resolve against. Both default to
 * `false` in the server AND in the shipped VS Code client.
 */
export const EDITOR_NEUTRAL_SERVER_PROFILES: Readonly<
  Record<EditorNeutralServerProfile, Readonly<Record<string, unknown>>>
> = {
  default: {},
  "verter-native-semantics": {
    hover: { nativeSemantics: true },
    analysis: { enabled: true },
  },
};

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
  /**
   * Structured provider recommendation from `$/verter/typeProviderStatus`
   * (tsgo-preferred model): REQUIRED on tsserver-family serving, FORBIDDEN on
   * tsgo-family serving.
   */
  readonly recommendation?: {
    readonly preferred: string;
    readonly reason: string;
    readonly knownGaps: readonly string[];
  };
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
  /**
   * Constructs excised from the hover text before {@link forbiddenHoverPatterns}
   * run. Each is text Verter is SUPPOSED to emit that would otherwise trip a strict
   * pattern; everything outside them is held to the strict rule.
   */
  readonly hoverTextExemptions?: readonly HoverTextExemption[];
  readonly expectedCompletion?: string;
  readonly expectedDiagnosticCode?: number;
  readonly renameTo?: string;
  /** Server configuration this case runs against; absent means `"default"`. */
  readonly serverProfile?: EditorNeutralServerProfile;
  readonly providers: readonly EditorNeutralProviderRoute[];
}

/** The profile a case runs under, resolving the `"default"` default. */
export function contractServerProfile(
  testCase: EditorNeutralContractCase,
): EditorNeutralServerProfile {
  return testCase.serverProfile ?? "default";
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
  /** Contractual constructs excised before {@link forbiddenPublicHover} runs. */
  readonly publicHoverExemptions?: readonly HoverTextExemption[];
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

/**
 * `any` in a TYPE-ANNOTATION position (`name: any`).
 *
 * A component's public hover prints the FRAMEWORK's own type. Vue's
 * `DefineComponent<…>` carries `any` among its trailing default type ARGUMENTS
 * (`…, ComponentProvideOptions, true, {}, any>>`), so a bare `/\bany\b/` fires on
 * every correct Vue component hover and can never be satisfied — it asserts
 * nothing about Verter.
 *
 * This predicate covers ANNOTATION-position degradation: the binding's own type
 * (`const JavaScriptCase: any`), a prop's type (`label: any`), a member's type
 * (`$props: any`). Verified on captured real hovers from all three routes: Vue's
 * own `…, {}, any>>` does NOT match, while the real shared-tsgo degradation
 * `const vueTsLocal: any` DOES.
 *
 * It is deliberately NOT the whole guard, and annotation position is NOT the only
 * shape a degradation can take — Verter's own wildcard erasure is positional
 * (`DefineComponent<{}, {}, any>`, `crates/verter_tsc/src/checker.rs`). Positional
 * `any` is indistinguishable BY POSITION from the framework's own default type
 * arguments, so it is caught by the OTHER half of the same case instead: an
 * erased props surface loses the prop NAMES `requiredPublicHover` demands, and the
 * required-fragment check fails. The two halves are load-bearing together; neither
 * alone is sufficient, and the `unknown` twin below is scoped the same way.
 *
 * Surfaces that print no framework generic list (script-local hovers, the plain
 * TypeScript control) keep the stricter bare `/\bany\b/i`.
 */
const ANY_IN_ANNOTATION_POSITION = /:\s*any\b/i;

/**
 * Verter's contractual untyped-emit fallback member, VERBATIM.
 *
 * A component whose script declares no `defineEmits` renders exactly this member.
 * `unknown` is the SAFE top type there and is the contractual answer, so a bare
 * `/\bunknown\b/` fires on every correct component hover and asserts nothing about
 * Verter. Excising this one member and holding everything else to the strict rule
 * is the right polarity: an unforeseen degradation is rejected by default, whereas
 * enumerating degradation shapes can never be complete.
 *
 * It is an EXACT STRING, not a pattern, and that is the whole point. Any pattern
 * loose enough to be written comfortably over printed type text over-matches, and
 * an over-broad exemption deletes the evidence before the strict rule ever sees it
 * — silently greening a degraded hover. A `$emit:\s*\([^)]*\)\s*=>\s*void` shaped
 * pattern, for instance, also swallows a degraded `$emit: (event: unknown) => void`,
 * an emit-shaped member nested inside a degraded prop, and — because `[^)]*` stops
 * at the first inner paren — the leading half of
 * `$emit: (event: string, cb: (value: unknown) => void) => void`, carrying the only
 * `unknown` away with it.
 *
 * Consequences of exactness, both deliberate:
 *   - Removal is FIRST-OCCURRENCE only, so a second emit-like member is never
 *     stripped by the exemption for the first.
 *   - Any other rendering of the fallback — a different parameter name, a space
 *     before the colon, a destructured rest — is NOT excised and trips the strict
 *     rule. That is a LOUD, fixable failure that names this constant; the opposite
 *     error, a silently widened exemption, is the failure mode this field exists to
 *     prevent. If Verter or TypeScript ever changes the rendering, update this
 *     constant from the new captured hover.
 *
 * Exact bytes are necessary but NOT sufficient: identical bytes at the wrong
 * structural POSITION must not be excised either. See {@link HoverTextExemption}
 * for the sibling-depth anchor that supplies that half.
 */
const UNTYPED_EMIT_FALLBACK_MEMBER = "$emit: (event: string, ...args: unknown[]) => void";

/**
 * An exemption is EXACT TEXT plus a STRUCTURAL POSITION. Both halves are load-bearing.
 *
 * Exact text alone is still a substring match, so the same bytes nested inside an
 * unrelated type are removed too. That is not hypothetical — it silently accepts a
 * degraded props surface:
 *
 * ```
 * label: { $emit: (event: string, ...args: unknown[]) => void };
 * $emit: (event: string) => void;
 * ```
 *
 * The required prop NAME survives, the only `unknown` sits inside the WRONG prop
 * type and is excised, and the real `$emit` carries no `unknown` — so nothing trips
 * and a wholly wrong hover greens.
 *
 * {@link siblingMember} anchors the excision structurally: the exempted text is
 * removed only where it sits at the SAME BRACE DEPTH as that sibling — i.e. as a
 * peer member of the same object body, which is what "the contractual member" means.
 * In the counterexample the nested `$emit` is one brace deeper than `$props`, so it
 * is not excised and the strict rule sees the `unknown`.
 *
 * If the sibling is absent, NOTHING is excised — fail closed, never fail open.
 */
interface HoverTextExemption {
  /** Exact member text to excise, verbatim from captured output. */
  readonly member: string;
  /**
   * A member that must sit at the same brace depth as {@link member} for the
   * excision to apply — its structural peer in the same object body.
   */
  readonly siblingMember: string;
}

/**
 * The hover value with its markdown FENCE MARKERS treated as noise — everything
 * else is one analysable region.
 *
 * The obvious reading, "analyse only the contents of fenced blocks", is WRONG
 * against real output. tsgo and shared-tsgo return a hover whose fence closes after
 * the FIRST LINE, leaving the rest of the type — `$props`, `$emit`, every member
 * that matters — outside it:
 *
 * ```text
 * ```typescript
 * (alias) const JavaScriptCase: __OmitNew<DefineComponent<ExtractPropTypes<{
 * ```
 *
 * label: { … }; … $emit: (event: string, ...args: unknown[]) => void; …
 * ```
 *
 * Fence position is therefore a formatting artifact of how the provider chopped the
 * payload, not a boundary between code and prose. Excising the markers and keeping
 * the remainder as one region matches every route's shape.
 *
 * This still buys the thing the narrowing was for: with the markers removed, a
 * BACKTICK in the remaining text can only be a template-literal delimiter, so
 * `` sep: `$props:` `` and `` sep: `}` `` are handled exactly like their
 * double-quoted twins instead of being holes.
 */
function codeRegions(text: string): readonly { start: number; end: number }[] {
  const regions: { start: number; end: number }[] = [];
  const marker = /```[^\n]*\n?/g;
  let cursor = 0;
  for (let match = marker.exec(text); match !== null; match = marker.exec(text)) {
    if (match.index > cursor) regions.push({ start: cursor, end: match.index });
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) regions.push({ start: cursor, end: text.length });
  return regions.length > 0 ? regions : [{ start: 0, end: text.length }];
}

/**
 * Per-character brace depth over one code region, ignoring braces inside STRING
 * LITERALS.
 *
 * A naive counter is not merely imprecise here, it fails OPEN: a string-literal
 * TYPE containing a brace (`sep: "}"`, ordinary TypeScript that QuickInfo prints
 * verbatim) decrements the count and can equalise a NESTED member's depth with a
 * top-level one — licensing an excision that hides a degradation. Quote handling is
 * therefore load-bearing, not cosmetic.
 *
 * All three TypeScript delimiters count — `"`, `'` and BACKTICK — because the fence
 * MARKERS are removed before this runs, leaving no other meaning a backtick could
 * carry. That is what excising the markers bought: a template-literal type holding a
 * brace (`` sep: `}` ``) or a member-looking string (`` sep: `$props:` ``) is now
 * handled exactly like its double-quoted twin, where both were previously holes —
 * the second of them a FAIL-OPEN one that hid a real degradation.
 *
 * KNOWN LIMITATION, deliberately accepted: JSDoc prose is NOT separable from the
 * type, so an apostrophe in prose (`doesn't`) still opens a literal here. Fence
 * position cannot be used to tell them apart — tsgo closes its fence mid-type, so
 * "outside the fence" is where most of the type lives (see `codeRegions`). A single
 * apostrophe fails CLOSED (nothing is excised, a good hover is falsely rejected —
 * loud); a PAIR could bracket a span whose braces go uncounted. It is unreachable
 * for the cases that declare an exemption today: their captured hovers on all three
 * routes contain no apostrophe. Separating prose would need a real parser, and this
 * is a test predicate.
 *
 * Indices are ABSOLUTE into the original text, so callers can compare candidate
 * positions across the whole value without translating coordinates.
 */
interface TextStructureProfile {
  readonly depths: ReadonlyMap<number, number>;
  readonly inLiteral: ReadonlySet<number>;
}

function braceDepthProfile(
  text: string,
  region: { start: number; end: number },
): TextStructureProfile {
  const depths = new Map<number, number>();
  const inLiteral = new Set<number>();
  let depth = 0;
  let quote: string | null = null;
  for (let at = region.start; at < region.end; at += 1) {
    depths.set(at, depth);
    if (quote !== null) inLiteral.add(at);
    const ch = text[at];
    if (quote !== null) {
      // A backslash escapes the next character, so an escaped quote does not close.
      if (ch === "\\") {
        at += 1;
        depths.set(at, depth);
        inLiteral.add(at);
      } else if (ch === quote) {
        quote = null;
      }
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      quote = ch;
      // The opening delimiter itself belongs to the literal.
      inLiteral.add(at);
    } else if (ch === "{") depth += 1;
    else if (ch === "}") depth -= 1;
  }
  depths.set(region.end, depth);
  return { depths, inLiteral };
}

/**
 * Remove the FIRST occurrence of `member` that is a PEER of `siblingMember` — same
 * brace depth AND the same enclosing object body. Returns the text unchanged when
 * no such occurrence exists.
 *
 * Equal depth alone is not peerhood, and the difference is a real false-accept:
 * in `{ $props: … } & { label: string; <member> }` the member sits at the same
 * depth as `$props` but in a DIFFERENT body, so a depth-only rule excises it and a
 * hover whose only `unknown` lived there greens. Two positions share a body only if
 * the depth between them never drops BELOW their common depth — crossing a `}` down
 * to an outer level means the first body closed.
 *
 * Every occurrence of the sibling is considered, not just the first: with the
 * same-body requirement doing the real work, restricting to the first would only
 * false-REJECT when the instance body is not the first one printed.
 *
 * Both the member and the sibling must be REAL members, not text that merely reads
 * like one. Candidates are drawn only from CODE regions, and a candidate inside a
 * STRING LITERAL is discarded: `sep: "$props:"` — and equally `` sep: `$props:` `` —
 * is valid TypeScript that QuickInfo prints verbatim, and counting it as a sibling
 * manufactures peerhood at whatever depth the literal happens to sit, authorising an
 * excision that removes the hover's only `unknown`. The same filter applies to the
 * member, so contractual bytes quoted inside a literal are never excised either.
 *
 * Peerhood is evaluated WITHIN a region: two members in different fenced blocks are
 * not peers, and prose mentions are not candidates at all.
 */
function exciseMemberAtSiblingDepth(text: string, exemption: HoverTextExemption): string {
  for (const region of codeRegions(text)) {
    const { depths, inLiteral } = braceDepthProfile(text, region);
    const within = (at: number): boolean => at >= region.start && at + 1 <= region.end;
    const candidates = (needle: string): number[] => {
      const found: number[] = [];
      for (
        let at = text.indexOf(needle, region.start);
        at >= 0;
        at = text.indexOf(needle, at + 1)
      ) {
        if (at + needle.length > region.end) break;
        if (within(at) && !inLiteral.has(at)) found.push(at);
      }
      return found;
    };

    const siblingIndices = candidates(exemption.siblingMember);
    if (siblingIndices.length === 0) continue;

    for (const at of candidates(exemption.member)) {
      const memberDepth = depths.get(at)!;
      const sharesBody = siblingIndices.some((siblingIndex) => {
        if (depths.get(siblingIndex) !== memberDepth) return false;
        const from = Math.min(siblingIndex, at);
        const to = Math.max(siblingIndex, at);
        for (let scan = from; scan <= to; scan += 1) {
          if (depths.get(scan)! < memberDepth) return false;
        }
        return true;
      });
      if (sharesBody) {
        return text.slice(0, at) + text.slice(at + exemption.member.length);
      }
    }
  }
  return text;
}

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
    // `const vueTsLocal = 1` in a TYPESCRIPT block keeps the literal type `1`;
    // only `let` (the Svelte carriers) and JavaScript `const` widen to `number`.
    // The fragment is identifier-qualified so it pins the resolved type OF THIS
    // BINDING: it fails on an empty hover, on `vueTsLocal: any`, and on a hover
    // that resolved some other symbol — where a bare `"number"` would have been
    // satisfied by any incidental mention (`toFixed(fractionDigits?: number)`).
    requiredLocalHover: ["vueTsLocal: 1"],
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
    // Vue's `DefineComponent<…>` prints `any` among its own trailing default
    // type ARGUMENTS, so only annotation-position `any` indicts Verter here.
    forbiddenPublicHover: [ANY_IN_ANNOTATION_POSITION, /\bunknown\b/i, /__Verter\w*/],
    // Verter is SUPPOSED to emit the untyped-emit fallback for a component with no
    // `defineEmits`; every other `unknown` in the printed type is a degradation.
    publicHoverExemptions: [{ member: UNTYPED_EMIT_FALLBACK_MEMBER, siblingMember: "$props:" }],
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
    // Vue's `DefineComponent<…>` prints `any` among its own trailing default
    // type ARGUMENTS, so only annotation-position `any` indicts Verter here.
    forbiddenPublicHover: [ANY_IN_ANNOTATION_POSITION, /\bunknown\b/i, /__Verter\w*/],
    // Verter is SUPPOSED to emit the untyped-emit fallback for a component with no
    // `defineEmits`; every other `unknown` in the printed type is a degradation.
    publicHoverExemptions: [{ member: UNTYPED_EMIT_FALLBACK_MEMBER, siblingMember: "$props:" }],
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

const LAX_JSCONFIG_DOM_EVENT_HANDLERS: readonly LaxDomEventContractSpec[] = [
  {
    id: "vue-js-dom-event-policy-lax-jsconfig",
    framework: "vue",
    document: "src/policy/lax-jsconfig/vue/JsConfigEventHandler.vue",
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
    id: "svelte-js-dom-event-policy-lax-jsconfig",
    framework: "svelte",
    document: "src/policy/lax-jsconfig/svelte/JsConfigEventHandler.svelte",
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
        hoverTextExemptions: carrier.publicHoverExemptions,
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
        hoverTextExemptions: carrier.publicHoverExemptions,
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

  // CSS class intelligence: markup class ↔ component style navigation
  // (Vue scoped styles + Svelte scoped-by-default styles), typed v-bind()
  // hover, and the fail-closed no-rule boundary. Verter-native results —
  // asserted identically on every provider route (no provider shadowing).
  {
    // CSS class intelligence is a Verter-native, OPT-IN lane: it needs both
    // `hover.nativeSemantics` and `analysis.enabled`, and the server and the
    // shipped VS Code client both default them off. These cases therefore run on
    // the `verter-native-semantics` profile — they assert the documented opt-in,
    // not the default. `css.default-off` below pins what the DEFAULT does, so
    // neither side of that boundary can move unnoticed.
    const cssProfile = "verter-native-semantics" as const;
    const vueCss = {
      framework: "vue" as const,
      language: "ts" as const,
      providers: ALL_PROVIDERS,
      surface: "standard-lsp" as const,
      serverProfile: cssProfile,
      document: "src/vue/CssIntel.vue",
    };
    const svelteCss = {
      framework: "svelte" as const,
      language: "ts" as const,
      providers: ALL_PROVIDERS,
      surface: "standard-lsp" as const,
      serverProfile: cssProfile,
      document: "src/svelte/CssIntel.svelte",
    };
    cases.push(
      {
        ...vueCss,
        id: "vue-css.class.hover",
        feature: "hover",
        anchor: {
          text: '<div class="chip-live ghost-none">',
          occurrence: 0,
          token: "chip-live",
        },
        requiredHoverFragments: ["```css", ".chip-live"],
      },
      {
        ...vueCss,
        id: "vue-css.class.definition",
        feature: "definition",
        anchor: {
          text: '<div class="chip-live ghost-none">',
          occurrence: 0,
          token: "chip-live",
        },
        expectedDefinitionDocument: "src/vue/CssIntel.vue",
        expectedDefinitionAnchor: {
          text: ".chip-live {",
          occurrence: 0,
          token: "chip-live",
        },
      },
      {
        ...vueCss,
        id: "vue-css.class.no-rule.silent",
        feature: "css-class-silent",
        anchor: {
          text: '<div class="chip-live ghost-none">',
          occurrence: 0,
          token: "ghost-none",
        },
      },
      {
        ...vueCss,
        id: "vue-css.vbind.hover",
        feature: "hover",
        anchor: {
          text: "width: v-bind(chipWidth);",
          occurrence: 0,
          token: "chipWidth",
        },
        requiredHoverFragments: ["v-bind(chipWidth)", "chipWidth: 12"],
      },
      {
        ...svelteCss,
        id: "svelte-css.class.hover",
        feature: "hover",
        anchor: {
          text: '<div class="chip-live" class:on>',
          occurrence: 0,
          token: "chip-live",
        },
        requiredHoverFragments: ["```css", ".chip-live"],
      },
      {
        ...svelteCss,
        id: "svelte-css.class.definition",
        feature: "definition",
        anchor: {
          text: '<div class="chip-live" class:on>',
          occurrence: 0,
          token: "chip-live",
        },
        expectedDefinitionDocument: "src/svelte/CssIntel.svelte",
        expectedDefinitionAnchor: {
          text: ".chip-live {",
          occurrence: 0,
          token: "chip-live",
        },
      },
      {
        ...svelteCss,
        id: "svelte-css.class-directive.definition",
        feature: "definition",
        anchor: {
          text: "class:on>",
          occurrence: 0,
          token: "on",
        },
        expectedDefinitionDocument: "src/svelte/CssIntel.svelte",
        expectedDefinitionAnchor: {
          text: ".on {",
          occurrence: 0,
          token: "on",
        },
      },
      // DEFAULT-configuration pins, one per opt-in lane moved above and each on the
      // SAME anchor its opt-in case asserts. Without them, moving CSS class
      // intelligence onto `verter-native-semantics` would leave NOTHING asserting
      // what an editor that wires no options actually gets.
      //
      // Each pin holds ONE direction: the default HOVER must stay silent. The
      // opposite direction — the opt-in going dark — is held by the paired opt-in
      // case on the same anchor (`vue-css.class.hover`, `svelte-css.class.hover`,
      // `vue-css.vbind.hover`). It is the PAIR that makes the boundary immovable;
      // neither case detects both directions by itself.
      //
      // The pins cover HOVER only, and that scope is a limitation, NOT a statement
      // that the current definition behaviour is correct. Under the DEFAULT profile
      // a Svelte class token resolves to its CSS rule while the Vue one does not —
      // and neither option is documented to gate definition at all
      // (`verter.hover.nativeSemantics` is scoped to hover;
      // `verter.analysis.enabled` is the Analysis sidebar, "TypeScript IDE features
      // remain independent"), while definition dispatch is unconditional. The Vue
      // side is therefore a suspected PRODUCT DEFECT, reported separately; it is
      // deliberately not pinned here, because pinning it would ratify it as
      // expected behaviour and make the eventual fix look like a regression.
      //
      // Known limitation, deliberately not covered: the lane needs BOTH
      // `hover.nativeSemantics` and `analysis.enabled`, so a pin cannot attribute a
      // change to one option. Detecting a single-option flip would need a profile per
      // option, and each profile costs one more server per route.
      {
        ...vueCss,
        serverProfile: "default",
        id: "vue-css.class.default-off",
        feature: "verter-native-default-off",
        anchor: {
          text: '<div class="chip-live ghost-none">',
          occurrence: 0,
          token: "chip-live",
        },
      },
      {
        ...svelteCss,
        serverProfile: "default",
        id: "svelte-css.class.default-off",
        feature: "verter-native-default-off",
        anchor: {
          text: '<div class="chip-live" class:on>',
          occurrence: 0,
          token: "chip-live",
        },
      },
      {
        ...vueCss,
        serverProfile: "default",
        id: "vue-css.vbind.default-off",
        feature: "verter-native-default-off",
        anchor: {
          text: "width: v-bind(chipWidth);",
          occurrence: 0,
          token: "chipWidth",
        },
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

  // D7: the same lax-JS carrier family, but configured by `jsconfig.json`
  // (never a `tsconfig.json`) — the exact project shape of the reported
  // js-lax repro. A plain `.js` sibling carrying the SAME JSDoc-annotated
  // member-access shape is the discriminating control: it hovers through the
  // ungated plain-file lane, while the carriers exercise the jsconfig-owned
  // carrier binding — the A/B isolates the carrier lane, never the typing
  // mechanism.
  for (const handler of LAX_JSCONFIG_DOM_EVENT_HANDLERS) {
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
  cases.push({
    id: "plain-js-lax-jsconfig-control.hover",
    surface: "standard-lsp",
    feature: "plain-control-hover",
    document: "src/policy/lax-jsconfig/plain-control.js",
    anchor: {
      text: "e.pointerId;",
      occurrence: 0,
      token: "e.pointerId",
    },
    requiredHoverFragments: ["PointerEvent"],
    forbiddenHoverPatterns: [/\bany\b/i, /\bunknown\b/i],
    providers: ALL_PROVIDERS,
  });

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
      // `export const plainControlNumber = 1` in TypeScript keeps the literal
      // type `1`. Identifier-qualified so the control still fails on an empty
      // hover, on `plainControlNumber: any`, and on a hover that resolved some
      // other symbol — see the `vue-ts` carrier note for the full rationale.
      requiredHoverFragments: ["plainControlNumber: 1"],
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
      // Forbidden patterns run against the text with each declared CONTRACTUAL
      // construct excised VERBATIM, first occurrence only. Describing the exemption
      // exactly and forbidding everything else fails in the safe direction: if the
      // exempted text ever changes, the excision stops matching and the strict
      // pattern fires — a loud failure rather than a silently widened predicate.
      // The reverse (describing every degradation shape) cannot be complete.
      const exemptions = testCase.hoverTextExemptions ?? [];
      const applied: string[] = [];
      const scanned = exemptions.reduce((remaining, exemption) => {
        // Exact bytes AND structural position: the member is removed only where it
        // is a peer of its declared sibling, so identical bytes nested inside an
        // unrelated type stay visible to the forbidden patterns.
        const excised = exciseMemberAtSiblingDepth(remaining, exemption);
        if (excised !== remaining) applied.push(exemption.member);
        return excised;
      }, text);
      for (const pattern of testCase.forbiddenHoverPatterns ?? []) {
        if (pattern.test(scanned)) {
          // Name the exemptions that did NOT apply. When a contractual construct is
          // re-rendered (a new TypeScript printing, a changed synthesized
          // signature), the strict pattern fires on text that is actually fine, and
          // this is the line that says so instead of leaving a bare "forbidden".
          const unapplied = exemptions
            .filter((exemption) => !applied.includes(exemption.member))
            .map((exemption) => exemption.member);
          const hint =
            unapplied.length === 0
              ? ""
              : ` (declared exemption(s) not excised — either absent verbatim, or present only ` +
                `at a nested position rather than as a peer of their sibling member; update them ` +
                `from captured output if the contractual rendering changed: ` +
                `${JSON.stringify(unapplied)})`;
          throw new Error(`${testCase.id}: hover matched forbidden ${pattern}${hint}: ${text}`);
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
    case "css-class-silent": {
      const position = positionFor(testCase, driver, sources);
      const hover = await driver.hover(testCase.document, position);
      const text = hoverText(hover);
      if (text.trim().length > 0) {
        throw new Error(
          `${testCase.id}: a rule-less class token must produce NO hover, got: ${text}`,
        );
      }
      const definition = await driver.definition(testCase.document, position);
      const locations = definitionLocations(definition);
      if (locations.length > 0) {
        throw new Error(
          `${testCase.id}: a rule-less class token must produce NO definition, got ${locations.length} location(s)`,
        );
      }
      return;
    }
    case "verter-native-default-off": {
      // A case that asserts an absence has to prove it is running the intended
      // configuration, or a mis-declared profile would make it pass for the wrong
      // reason. Refuse to execute on anything but `default`.
      if (contractServerProfile(testCase) !== "default") {
        throw new Error(
          `${testCase.id}: a default-configuration pin must declare the "default" server profile, ` +
            `got "${contractServerProfile(testCase)}"`,
        );
      }
      // HOVER only — the option this pin names is scoped to hover. Definition is
      // deliberately NOT asserted here, and its current behaviour is NOT ratified:
      // under the DEFAULT profile a Svelte class token resolves to its CSS rule
      // while the Vue one does not, even though neither option is documented to
      // gate definition. See the pin definitions for why that asymmetry is treated
      // as a suspected product defect rather than as expected behaviour.
      const position = positionFor(testCase, driver, sources);
      const hover = await driver.hover(testCase.document, position);
      const text = hoverText(hover);
      if (text.trim().length > 0) {
        throw new Error(
          `${testCase.id}: the DEFAULT configuration must not answer this opt-in hover, but ` +
            `hover returned: ${text}. If the default was deliberately flipped on, this pin and ` +
            "the shipped client default must move together.",
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
        // tsgo-preferred flip: tsserver-family serving MUST carry the
        // structured TSGO recommendation with honest, non-empty known gaps
        // and editor-agnostic wording (no client settings keys server-side).
        const recommendation = attestation.recommendation;
        if (!recommendation || recommendation.preferred !== "tsgo") {
          throw new Error(
            `${testCase.id}: tsserver serving must recommend tsgo, got ${JSON.stringify(recommendation)}`,
          );
        }
        if (recommendation.knownGaps.length === 0) {
          throw new Error(`${testCase.id}: recommendation must disclose known gaps honestly`);
        }
        const portable = [recommendation.reason, ...recommendation.knownGaps];
        if (portable.some((text) => text.includes("VS Code") || text.includes("verter."))) {
          throw new Error(
            `${testCase.id}: recommendation wording must be editor-agnostic: ${JSON.stringify(portable)}`,
          );
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
      // Negative (tsgo-family routes): the server never nags users already on
      // the preferred provider — no recommendation, and no retired
      // "known limitations" warning content.
      if (driver.route !== "tsserver" && attestation.recommendation !== undefined) {
        throw new Error(
          `${testCase.id}: tsgo-family serving must carry no recommendation, got ${JSON.stringify(attestation.recommendation)}`,
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
