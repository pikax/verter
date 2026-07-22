/**
 * The TypeScript-answer discriminator.
 *
 * The acceptance lane exists to answer one question: **does VS Code show the
 * results from the TypeScript server?** Every other signal in this repository
 * measures the language server through a harness; this lane measures the
 * editor. That makes a false green here worse than no measurement at all, so
 * this module is the single authority on what counts as a TypeScript answer,
 * and it is deliberately paranoid.
 *
 * ## Why a naive discriminator produces false greens
 *
 * Verter answers hovers natively when the type provider cannot. One of those
 * native formatters (`crates/verter_lsp/src/features/hover.rs:1650`
 * `format_binding_hover`) emits, verbatim:
 *
 * ```text
 * ```typescript
 * const total: Money
 * ```
 * ```
 *
 * That is byte-for-byte the shape of a real `tsserver`/`tsgo` quickinfo hover
 * for a `const` declaration. "The hover contains a declaration with a type
 * position" therefore does NOT discriminate — it accepts a Verter-native
 * answer. Any lane built on that rule reports green while the TypeScript engine
 * is completely absent, which is precisely the failure this lane was created to
 * catch.
 *
 * ## What actually discriminates
 *
 * Two independent rails, both required:
 *
 * 1. **A TypeScript-exclusive marker.** `tsserver` and `tsgo` prefix quickinfo
 *    for members, aliases, parameters and locals with a parenthesised kind
 *    token at the very start of the code fence — `(property)`, `(alias)`,
 *    `(method)`, `(parameter)`, … . No Verter-native hover formatter emits a
 *    fence beginning with `(`; every one of them starts with `const`, `let`,
 *    `var`, `function`, `class`, `import`, or a macro name. This is a
 *    structural property of the producers, not a heuristic about wording.
 *
 * 2. **A probe class whose native answer is provably different.** The lane only
 *    probes positions where the native formatter cannot produce the accepted
 *    shape:
 *
 *    - `member` — a `.property` access. `hover_for_word` (hover.rs:1601) looks
 *      symbols up by whole-word match against bindings, imports and macros, so
 *      it has no answer for a property of a type. TypeScript answers
 *      `(property) Foo.bar: T`.
 *    - `alias` — an imported specifier. Native emits a bare re-print of the
 *      import statement (`format_import_hover`, hover.rs:1730). TypeScript
 *      answers `(alias) …`.
 *    - `inferred-local` — a local declared WITHOUT an authored type annotation.
 *      Native can only print the authored annotation, so with none present it
 *      emits `const x` with no type at all; a type in that position can only
 *      have been inferred by the TypeScript engine. This class is accepted on
 *      the type-position rail ONLY when the caller proves the declaration
 *      carries no authored annotation (`declarationHasNoAuthoredAnnotation`);
 *      an unproven probe falls back to requiring rail 1.
 *
 * A hover that matches a known Verter-native fingerprint is reported as
 * `verter-native` rather than merely "not TypeScript", so the lane can
 * distinguish "the engine did not answer" from "nothing answered at all".
 */

/**
 * Parenthesised quickinfo kind tokens emitted by TypeScript's
 * `SymbolDisplayPart` builder at the head of a quickinfo string.
 *
 * No Verter-native hover formatter can emit a code fence whose first line
 * begins with `(` — see the module docs. This list is therefore the
 * TypeScript-exclusive rail.
 */
export const TS_QUICKINFO_KINDS: readonly string[] = [
  "alias",
  "await",
  "call",
  "construct",
  "constructor",
  "enum member",
  "getter",
  "index",
  "JSX attribute",
  "local class",
  "local function",
  "local var",
  "method",
  "parameter",
  "property",
  "setter",
  "type parameter",
];

/**
 * Literal fragments that only Verter's own hover formatters produce.
 *
 * Each entry cites the producer so a future change to the Rust side can be
 * traced back here. These are used to REPORT a native answer, never to accept
 * one.
 */
export const VERTER_NATIVE_HOVER_FINGERPRINTS: readonly string[] = [
  // format_import_hover — hover.rs:1741
  "Vue API: `",
  // format_macro_hover — hover.rs:1780
  "Type-based: `<",
  // format_binding_hover reactivity markers — hover.rs:1678-1695
  "*(ref — needs `.value`)*",
  "*(computed — needs `.value`, read-only)*",
  "*(reactive — direct property access)*",
  "*(maybe ref — may need `.value`)*",
  "*(mutable — reassignable)*",
  "*(reactive)*",
  // format_binding_hover initializer lines — hover.rs:1709-1716
  "Initialized via `",
  "References `",
  "Literal: ",
  // block / slot / component summaries — hover.rs:288, 896, 944, 1438
  "— Custom block.",
  "**`<slot>`** outlet",
  "**Slot content** —",
  "**Slot** `",
];

/** Which position the probe was taken at. See the module docs. */
export type ProbeClass = "member" | "alias" | "inferred-local";

/** The four outcomes a probed IDE operation can have. */
export type AnswerVerdict = "typescript" | "verter-native" | "empty" | "indeterminate";

export interface HoverProbeContract {
  readonly probeClass: ProbeClass;
  /** The identifier under the cursor. */
  readonly identifier: string;
  /**
   * `inferred-local` only. MUST be proven by the probe selector by inspecting
   * the declaration source. When this is not `true` the type-position rail is
   * disabled and only a TypeScript-exclusive quickinfo prefix is accepted, so
   * an unverified probe can never manufacture a green.
   */
  readonly declarationHasNoAuthoredAnnotation?: boolean;
}

export interface HoverVerdict {
  readonly verdict: AnswerVerdict;
  /** Human-readable justification, safe to print (carries no corpus text). */
  readonly reason: string;
  /** The token that decided a positive verdict. */
  readonly marker?: string;
}

const FENCE_RE = /```(?:typescript|ts|tsx|javascript|js)\r?\n([\s\S]*?)```/g;

/** Extract the bodies of TypeScript-ish fenced code blocks, in order. */
export function extractCodeFences(markdown: string): string[] {
  const fences: string[] = [];
  FENCE_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = FENCE_RE.exec(markdown)) !== null) {
    fences.push(match[1].replace(/\s+$/, ""));
  }
  return fences;
}

/**
 * Return the TypeScript quickinfo kind token when `fence` begins with one.
 *
 * The token must be at the very start of the fence's first line — that is
 * where TypeScript puts it, and no Verter-native formatter starts a fence with
 * `(`.
 */
export function quickInfoPrefix(fence: string): string | undefined {
  const firstLine = fence.split(/\r?\n/, 1)[0] ?? "";
  const match = /^\(([^)]+)\)/.exec(firstLine.trimStart());
  if (!match) return undefined;
  return TS_QUICKINFO_KINDS.includes(match[1]) ? match[1] : undefined;
}

/** Return the first Verter-native fingerprint present in `text`, if any. */
export function verterNativeFingerprint(text: string): string | undefined {
  return VERTER_NATIVE_HOVER_FINGERPRINTS.find((needle) => text.includes(needle));
}

/**
 * True when the fence is a bare re-print of an import statement, which is what
 * `format_import_hover` (hover.rs:1730) emits and what an `(alias)` quickinfo
 * never is.
 */
export function isBareImportReprint(fence: string): boolean {
  const lines = fence.split(/\r?\n/).filter((line) => line.trim().length > 0);
  if (lines.length !== 1) return false;
  return /^import\s/.test(lines[0].trim()) && / from ['"]/.test(lines[0]);
}

/**
 * Return the inferred type text when the fence declares `identifier` WITH a
 * type position, e.g. `const total: Money` → `Money`.
 */
export function declaredTypeText(fence: string, identifier: string): string | undefined {
  const escaped = identifier.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(
    `^(?:const|let|var|function|async function|class)\\s+${escaped}\\s*:\\s*(\\S.*)$`,
    "m",
  );
  const match = re.exec(fence);
  const text = match?.[1]?.trim();
  return text && text.length > 0 ? text : undefined;
}

/**
 * Classify a hover payload for a probe of a known class.
 *
 * See the module docs for why the rails are ordered this way. In particular the
 * TypeScript-exclusive rail is evaluated BEFORE the native fingerprints, so a
 * genuine quickinfo whose JSDoc happens to quote one of Verter's strings is
 * still credited to TypeScript.
 */
export function classifyHoverText(text: string, probe: HoverProbeContract): HoverVerdict {
  if (text.trim().length === 0) {
    return { verdict: "empty", reason: "hover returned no content" };
  }

  const fences = extractCodeFences(text);

  // Rail 1 — TypeScript-exclusive quickinfo prefix.
  for (const fence of fences) {
    const kind = quickInfoPrefix(fence);
    if (kind) {
      return {
        verdict: "typescript",
        reason: `quickinfo kind prefix (${kind})`,
        marker: `(${kind})`,
      };
    }
  }

  const native = verterNativeFingerprint(text);
  if (native) {
    return { verdict: "verter-native", reason: "Verter-native hover fingerprint", marker: native };
  }

  if (fences.length > 0 && fences.every(isBareImportReprint)) {
    return {
      verdict: "verter-native",
      reason: "bare import re-print (format_import_hover), not an (alias) quickinfo",
      marker: "import …",
    };
  }

  // Rail 2 — a type position the native formatter provably cannot produce.
  if (probe.probeClass === "inferred-local") {
    if (probe.declarationHasNoAuthoredAnnotation !== true) {
      return {
        verdict: "indeterminate",
        reason:
          "inferred-local probe was not proven annotation-free; the type-position rail is " +
          "disabled because Verter-native hover can re-print an authored annotation",
      };
    }
    for (const fence of fences) {
      const typeText = declaredTypeText(fence, probe.identifier);
      if (typeText) {
        return {
          verdict: "typescript",
          // The marker is a fixed token, never the type text: markers travel
          // into receipts and reports, and the corpora this lane runs against
          // are private.
          reason: `type on a declaration with no authored annotation — necessarily inferred (${typeText.length} chars)`,
          marker: "inferred-type",
        };
      }
    }
  }

  if (fences.length === 0) {
    return { verdict: "indeterminate", reason: "no TypeScript code fence in hover" };
  }
  return {
    verdict: "indeterminate",
    reason: `no TypeScript-exclusive marker for a ${probe.probeClass} probe`,
  };
}

// ── Non-hover operations ───────────────────────────────────────────────────
//
// Hover is the ONLY operation whose payload can be attributed to the TypeScript
// engine from the payload alone, because quickinfo carries a kind prefix no
// Verter-native formatter produces.
//
// Definition, completion and references return structural payloads — locations
// and labels — that carry no engine signature. That is not a theoretical
// concern: running this lane with `verter.typeProvider = off`, so that NO
// engine exists at all, still produced cross-file definitions into `.ts` files,
// member completions, and cross-file references, because Verter answers those
// natively. A classifier that credited "landed in a .ts file" to TypeScript
// therefore reported the engine as present on a run where it was disabled.
//
// So these three deliberately CANNOT return `typescript`. They report whether
// the operation resolved, and attribution is left to a paired control run.
// Encoding that in the type is what stops the mistake from recurring: there is
// no value a caller can read as "the engine answered".

/** Outcome of an operation whose payload cannot be attributed to an engine. */
export type ResolutionVerdict = "resolved" | "unresolved" | "empty";

export interface ResolutionOutcome {
  readonly verdict: ResolutionVerdict;
  readonly reason: string;
  readonly marker?: string;
}

/** File extensions whose contents are owned by the TypeScript program. */
const TS_PROGRAM_EXTENSIONS = [".ts", ".tsx", ".mts", ".cts", ".d.ts"];

export function isTypeScriptProgramFile(fsPath: string): boolean {
  const lower = fsPath.toLowerCase();
  return TS_PROGRAM_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

export interface DefinitionVerdictInput {
  readonly targetPaths: readonly string[];
  /** The carrier the probe was taken in, so a self-landing can be identified. */
  readonly sourcePath: string;
}

/**
 * Did go-to-definition resolve, and did it leave the probe file?
 *
 * This deliberately does NOT report which component answered. A control run
 * with the type provider disabled still produced cross-file definitions into
 * `.ts` files, so "landed in the TypeScript program" is evidence of resolution,
 * not of the engine.
 */
export function classifyDefinition(input: DefinitionVerdictInput): ResolutionOutcome {
  if (input.targetPaths.length === 0) {
    return { verdict: "empty", reason: "definition returned no locations" };
  }
  const crossFile = input.targetPaths.filter(
    (p) => p.toLowerCase() !== input.sourcePath.toLowerCase(),
  );
  const inProgram = crossFile.filter(isTypeScriptProgramFile);
  if (inProgram.length > 0) {
    return {
      verdict: "resolved",
      reason: "definition landed in a TypeScript program file",
      marker: "ts-target",
    };
  }
  if (crossFile.length > 0) {
    return {
      verdict: "resolved",
      reason: "definition landed cross-file outside the TypeScript program",
      marker: "cross-file",
    };
  }
  return { verdict: "unresolved", reason: "definition resolved only within the probe file" };
}

export interface CompletionItemFacts {
  readonly label: string;
  /** `vscode.CompletionItemKind` numeric value. */
  readonly kind?: number;
  readonly detail?: string;
}

/**
 * Did member completion offer anything the probe file does not itself declare?
 *
 * Same caveat as `classifyDefinition`: the provider-disabled control run also
 * produced foreign member labels, so this measures usefulness, not provenance.
 */
export function classifyMemberCompletion(
  items: readonly CompletionItemFacts[],
  probeSourceText: string,
): ResolutionOutcome {
  if (items.length === 0) {
    return { verdict: "empty", reason: "completion returned no items" };
  }
  const foreign = items.filter(
    (item) => item.label.length > 1 && !probeSourceText.includes(item.label),
  );
  if (foreign.length > 0) {
    return {
      verdict: "resolved",
      reason: `${foreign.length}/${items.length} members are not declared in the probe file`,
      marker: "foreign-member",
    };
  }
  return {
    verdict: "unresolved",
    reason: "every completion label already occurs in the probe file",
  };
}

export interface ReferenceVerdictInput {
  readonly locationPaths: readonly string[];
  readonly sourcePath: string;
}

/** Did find-all-references resolve, and did it cross a file boundary? */
export function classifyReferences(input: ReferenceVerdictInput): ResolutionOutcome {
  if (input.locationPaths.length === 0) {
    return { verdict: "empty", reason: "references returned no locations" };
  }
  const crossFile = input.locationPaths.filter(
    (p) => p.toLowerCase() !== input.sourcePath.toLowerCase(),
  );
  if (crossFile.length > 0) {
    return {
      verdict: "resolved",
      reason: `${crossFile.length} cross-file reference(s)`,
      marker: "cross-file",
    };
  }
  return {
    verdict: "resolved",
    reason: `${input.locationPaths.length} same-file reference(s) only`,
    marker: "same-file",
  };
}
