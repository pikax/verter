/**
 * Selection of `textDocument/definition` targets from a raw LSP response.
 *
 * A definition response is a `Location`, a `LocationLink`, or an array of either,
 * and its targets may point at ANY document. Selecting them is small but not
 * trivial enough to inline in a gate script: a probe that compares only LINE
 * NUMBERS is satisfied by the right line in the wrong file, which is exactly the
 * class of near-miss a definition assertion exists to catch.
 */

/** One resolved definition target in document coordinates. */
export interface DefinitionTarget {
  /** The document the target lives in. */
  readonly uri: string;
  /** 0-based line of the target's start position. */
  readonly line: number;
  /** 0-based character of the target's start position. */
  readonly character: number;
}

/**
 * Flatten a definition response into typed targets.
 *
 * Accepts both shapes the protocol allows: a `Location` (`uri` + `range`) and a
 * `LocationLink` (`targetUri` + `targetSelectionRange`/`targetRange`). Entries
 * without a usable uri/range are dropped rather than guessed at.
 */
export function definitionTargets(result: unknown): readonly DefinitionTarget[] {
  if (result === null || result === undefined) return [];
  const entries = Array.isArray(result) ? result : [result];
  const targets: DefinitionTarget[] = [];
  for (const raw of entries) {
    if (raw === null || typeof raw !== "object") continue;
    const entry = raw as Record<string, unknown>;
    const uri = entry.targetUri ?? entry.uri;
    const range = (entry.targetSelectionRange ?? entry.targetRange ?? entry.range) as
      | { start?: { line?: number; character?: number } }
      | undefined;
    if (typeof uri !== "string" || range?.start === undefined) continue;
    targets.push({
      uri,
      line: range.start.line ?? 0,
      character: range.start.character ?? 0,
    });
  }
  return targets;
}

/**
 * Partition targets by document.
 *
 * The split is the point: a caller asserting "definition resolved line N" must
 * compare lines only WITHIN the expected document, and needs the out-of-document
 * targets to report rather than silently ignore. Returning both halves makes the
 * near-miss — right line, wrong file — impossible to state as a pass.
 */
export function partitionDefinitionTargets(
  targets: readonly DefinitionTarget[],
  uri: string,
): {
  readonly inDocument: readonly DefinitionTarget[];
  readonly elsewhere: readonly DefinitionTarget[];
} {
  return {
    inDocument: targets.filter((target) => target.uri === uri),
    elsewhere: targets.filter((target) => target.uri !== uri),
  };
}
