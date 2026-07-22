/**
 * Probe selection for the VS Code acceptance lane.
 *
 * The lane runs against real projects supplied at the shell, so it cannot ship
 * hand-picked cursor positions: it has to derive them from whatever source it
 * finds. These selectors are pure text functions over file contents so they can
 * be proven hermetically, without a VS Code host and without a corpus.
 *
 * Two contracts matter here, and both feed the discriminator in `tsAnswer.ts`:
 *
 * - An `alias` or `member` probe only needs a position; the discriminator
 *   rejects native answers at those positions on its own rails.
 * - An `inferred-local` probe additionally carries
 *   `declarationHasNoAuthoredAnnotation`. That flag UNLOCKS the discriminator's
 *   type-position rail, so a selector that sets it wrongly manufactures false
 *   greens. `findInferredLocalProbes` therefore only ever emits declarations it
 *   has confirmed have no `:` annotation between the binding name and the `=`.
 *
 * Nothing here records file names, paths, or source text — see `redactPath`.
 */
import { createHash } from "node:crypto";

import type { ProbeClass } from "./tsAnswer";

export interface SourceProbe {
  readonly probeClass: ProbeClass;
  readonly identifier: string;
  /** Zero-based UTF-16 offset of the identifier within the file. */
  readonly offset: number;
  /** `inferred-local` only — proven by inspecting the declaration. */
  readonly declarationHasNoAuthoredAnnotation?: boolean;
}

/** Identifiers that never make useful probes. */
const RESERVED = new Set([
  "await",
  "case",
  "catch",
  "class",
  "const",
  "default",
  "delete",
  "else",
  "export",
  "from",
  "function",
  "if",
  "import",
  "in",
  "instanceof",
  "let",
  "new",
  "of",
  "return",
  "this",
  "throw",
  "typeof",
  "var",
  "void",
  "while",
  "yield",
]);

/**
 * Region of a `.vue` source that holds TypeScript. For a non-carrier file the
 * whole text is the region.
 */
export interface ScriptRegion {
  readonly start: number;
  readonly end: number;
}

const SCRIPT_OPEN_RE = /<script\b[^>]*>/gi;

/**
 * Return the `<script>` / `<script setup>` regions of a carrier, or the whole
 * file when it has no `<script>` tag (a plain `.ts`/`.js` file).
 */
export function scriptRegions(text: string): ScriptRegion[] {
  const regions: ScriptRegion[] = [];
  SCRIPT_OPEN_RE.lastIndex = 0;
  let open: RegExpExecArray | null;
  while ((open = SCRIPT_OPEN_RE.exec(text)) !== null) {
    const start = open.index + open[0].length;
    const close = text.indexOf("</script>", start);
    if (close < 0) break;
    regions.push({ start, end: close });
    SCRIPT_OPEN_RE.lastIndex = close;
  }
  if (regions.length === 0) return [{ start: 0, end: text.length }];
  return regions;
}

/** True when the file declares its script block as TypeScript. */
export function isTypeScriptCarrier(text: string): boolean {
  return /<script\b[^>]*\blang\s*=\s*["']ts["']/i.test(text);
}

function inRegions(offset: number, regions: readonly ScriptRegion[]): boolean {
  return regions.some((r) => offset >= r.start && offset < r.end);
}

/**
 * Reject offsets that sit inside a string literal or a line comment.
 *
 * Without this, a module specifier like `"./money.ts"` reads as a member access
 * on `money`, and the lane spends a probe hovering inside a string where
 * nothing can answer. Those positions are not false greens — they are false
 * EMPTIES, which is just as misleading in a lane whose thesis is that an empty
 * result is a defect.
 */
export function isInCodeContext(text: string, offset: number): boolean {
  const lineStart = text.lastIndexOf("\n", offset - 1) + 1;
  const prefix = text.slice(lineStart, offset);
  const comment = prefix.indexOf("//");
  if (comment >= 0) return false;
  const count = (needle: string) => prefix.split(needle).length - 1;
  return count('"') % 2 === 0 && count("'") % 2 === 0 && count("`") % 2 === 0;
}

/**
 * Named import specifiers, e.g. `import { formatMoney, type Money } from "./x"`.
 *
 * Type-only specifiers are skipped: they are legitimate `(alias)` targets but
 * their hover is more likely to be elided, and the lane wants the strongest
 * available probe rather than the broadest.
 */
export function findAliasProbes(text: string): SourceProbe[] {
  const regions = scriptRegions(text);
  const probes: SourceProbe[] = [];
  const importRe = /import\s+(?:type\s+)?\{([^}]*)\}\s*from\s*["'][^"']+["']/g;
  let match: RegExpExecArray | null;
  while ((match = importRe.exec(text)) !== null) {
    if (!inRegions(match.index, regions)) continue;
    if (/^import\s+type\b/.test(match[0])) continue;
    const clauseStart = match.index + match[0].indexOf("{") + 1;
    const specRe = /(^|,)\s*(type\s+)?([A-Za-z_$][\w$]*)/g;
    let spec: RegExpExecArray | null;
    while ((spec = specRe.exec(match[1])) !== null) {
      if (spec[2]) continue; // inline `type` specifier
      const identifier = spec[3];
      if (RESERVED.has(identifier)) continue;
      const offset = clauseStart + spec.index + spec[0].indexOf(identifier);
      probes.push({ probeClass: "alias", identifier, offset });
    }
  }
  return probes;
}

/**
 * Property accesses inside a script region, e.g. the `total` of `invoice.total`.
 *
 * `hover_for_word` resolves whole words against the file's bindings, imports and
 * macros, so it has no native answer for a property of a type — which is what
 * makes this the strongest probe class the lane has.
 */
export function findMemberProbes(text: string): SourceProbe[] {
  const regions = scriptRegions(text);
  const probes: SourceProbe[] = [];
  const memberRe = /([A-Za-z_$][\w$]*)\s*\.\s*([A-Za-z_$][\w$]*)/g;
  let match: RegExpExecArray | null;
  while ((match = memberRe.exec(text)) !== null) {
    if (!inRegions(match.index, regions)) continue;
    const property = match[2];
    if (RESERVED.has(property) || RESERVED.has(match[1])) continue;
    if (!isInCodeContext(text, match.index)) continue;
    const offset = match.index + match[0].lastIndexOf(property);
    probes.push({ probeClass: "member", identifier: property, offset });
  }
  return probes;
}

const DECL_RE = /\b(?:const|let)\s+([A-Za-z_$][\w$]*)\s*(:[^=;]*)?=\s*([^\n;]*)/g;

/**
 * Locals declared WITHOUT an authored type annotation.
 *
 * This is the only selector that unlocks the discriminator's type-position
 * rail, so it is deliberately conservative: a declaration is emitted only when
 * the regex captured NO annotation group at all. Destructuring patterns are
 * skipped because their binding names do not map to a single hover target.
 */
export function findInferredLocalProbes(text: string): SourceProbe[] {
  const regions = scriptRegions(text);
  const probes: SourceProbe[] = [];
  DECL_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = DECL_RE.exec(text)) !== null) {
    if (!inRegions(match.index, regions)) continue;
    const identifier = match[1];
    if (RESERVED.has(identifier)) continue;
    if (!isInCodeContext(text, match.index)) continue;
    // An annotation was authored — the native formatter can re-print it, so the
    // type-position rail would not discriminate. Refuse the probe.
    if (match[2] !== undefined) continue;
    // Only initializers that force inference are useful; a bare literal is
    // still fine, but an empty initializer is not a probe at all.
    if (match[3].trim().length === 0) continue;
    const offset = match.index + match[0].indexOf(identifier);
    probes.push({
      probeClass: "inferred-local",
      identifier,
      offset,
      declarationHasNoAuthoredAnnotation: true,
    });
  }
  return probes;
}

/**
 * Collect a bounded, deterministic probe set for one file.
 *
 * `perClass` caps each class so a large file cannot dominate a run. Selection
 * is first-N in source order so a rerun over unchanged sources probes exactly
 * the same positions.
 */
export function selectProbes(text: string, perClass = 3): SourceProbe[] {
  return [
    ...findMemberProbes(text).slice(0, perClass),
    ...findAliasProbes(text).slice(0, perClass),
    ...findInferredLocalProbes(text).slice(0, perClass),
  ];
}

/**
 * A stable, non-reversible identifier for a workspace-relative path.
 *
 * The corpora this lane runs against are private. Receipts, logs and reports
 * must never carry a project, package, or file name, so every path that leaves
 * the lane is reduced to a digest plus its extension — enough to correlate two
 * runs, not enough to identify anything.
 */
export function redactPath(relativePosixPath: string): string {
  const digest = createHash("sha256").update(relativePosixPath).digest("hex").slice(0, 12);
  const dot = relativePosixPath.lastIndexOf(".");
  const ext = dot > 0 ? relativePosixPath.slice(dot) : "";
  return `${digest}${ext}`;
}

/** Redact an identifier the same way, so probe names never leave the lane. */
export function redactIdentifier(identifier: string): string {
  return createHash("sha256").update(identifier).digest("hex").slice(0, 8);
}
