/**
 * The single producer-side redactor (TS side).
 *
 * Mirrors the Rust `verter_analysis_inputs::Redactor`. This is the ONE place a real
 * analysis-input path / identifier / verbatim message becomes an opaque token at
 * OUTPUT-PRODUCTION time — every campaign emitter (JSONL collectors, TSC diff /
 * review-queue, source-map projection, logs, ledger) routes its values through here
 * before writing. The redacted-emitter wrappers serialize already-redacted values,
 * so this is producer-side enforcement, not a post-write scrub.
 *
 * - {@link Redactor.redactValue} — replace any known real root prefix (and its
 *   project-relative remainder) with an opaque `analysis://<id>/file-<NNNN>.<ext>`.
 * - {@link Redactor.sourceMapSource} — opaque virtual id for a source map's
 *   `sources` entry; `sourcesContent` for corpus input is OMITTED entirely.
 * - {@link Redactor.redactDiagnostic} — SHAPE-redact a TS/Vue diagnostic message
 *   (identifiers, import paths, component names, string literals → a template
 *   shape) so a verbatim message never reaches an artifact.
 */

import type { AnalysisProjects } from "./analysisConfig.js";

/** Forward-slash-normalize + lowercase a leading Windows drive letter. */
function normalize(s: string): string {
  let out = s.replace(/\\/g, "/");
  out = out.replace(/^([A-Za-z]):/, (_m, d: string) => `${d.toLowerCase()}:`);
  return out;
}

/** One project's opaque id + normalized root (no trailing slash). */
interface RootEntry {
  readonly id: string;
  readonly normRoot: string;
}

/** The extension (no dot) of a relative path, or `bin`. */
function extOf(rel: string): string {
  const base = rel.split("/").pop() ?? rel;
  const dot = base.lastIndexOf(".");
  if (dot <= 0 || dot === base.length - 1) return "bin";
  return base.slice(dot + 1).toLowerCase();
}

export class Redactor {
  private readonly roots: RootEntry[];
  /** `id rel` → stable file number. */
  private readonly fileNumbers = new Map<string, number>();
  private readonly nextNumber = new Map<string, number>();

  /** Build from an explicit `id → root` map. */
  constructor(idRoot: ReadonlyArray<readonly [string, string]>) {
    this.roots = idRoot
      .map(([id, root]) => {
        let normRoot = normalize(root);
        while (normRoot.endsWith("/") && normRoot.length > 1) normRoot = normRoot.slice(0, -1);
        return { id, normRoot };
      })
      // Longest root first so a nested root wins over its ancestor.
      .sort((a, b) => b.normRoot.length - a.normRoot.length);
  }

  /** Build from a loaded config's `id → root` pairs. */
  static fromConfig(config: AnalysisProjects): Redactor {
    return new Redactor(config.projects.map((p) => [p.id, p.root] as const));
  }

  private fileNumber(id: string, rel: string): number {
    const key = `${id} ${rel}`;
    const existing = this.fileNumbers.get(key);
    if (existing !== undefined) return existing;
    const n = (this.nextNumber.get(id) ?? 0) + 1;
    this.nextNumber.set(id, n);
    this.fileNumbers.set(key, n);
    return n;
  }

  private pad(n: number): string {
    return String(n).padStart(4, "0");
  }

  /** If `normPath` is under a known root, return `{ id, rel }`. */
  private matchRoot(normPath: string): { id: string; rel: string } | null {
    for (const r of this.roots) {
      if (normPath === r.normRoot) return { id: r.id, rel: "" };
      const prefix = `${r.normRoot}/`;
      if (normPath.startsWith(prefix)) return { id: r.id, rel: normPath.slice(prefix.length) };
    }
    return null;
  }

  /**
   * The opaque virtual id for a real path (a source map's `sources` entry):
   * `analysis://<id>/file-<NNNN>.<ext>`. For a path under no known root, returns
   * `null` so the caller can FAIL CLOSED rather than emit an unrecognized path.
   */
  sourceMapSource(realPath: string): string | null {
    const m = this.matchRoot(normalize(realPath));
    if (m === null) return null;
    if (m.rel === "") return `analysis://${m.id}`;
    const n = this.fileNumber(m.id, m.rel);
    return `analysis://${m.id}/file-${this.pad(n)}.${extOf(m.rel)}`;
  }

  /** Opaque display form; an unknown-root path becomes `analysis://unknown`. */
  displayPath(realPath: string): string {
    return this.sourceMapSource(realPath) ?? "analysis://unknown";
  }

  /**
   * Redact every known-root occurrence inside an arbitrary string. Each real root
   * prefix (plus its `/relative/path` remainder) becomes an opaque virtual id, so
   * neither the root NOR the relative basename survives. Text with no known root
   * passes through unchanged (generic placeholders, `/tmp`, repo-relative paths).
   */
  redactValue(s: string): string {
    const norm = normalize(s);
    let out = "";
    let rest = norm;
    // Stop at a path-terminating character so we only consume the path token.
    const terminator = /[\s"'<>)\],;]/;
    outer: while (rest.length > 0) {
      for (const r of this.roots) {
        const at = rest.indexOf(r.normRoot);
        if (at >= 0) {
          out += rest.slice(0, at);
          const afterRoot = rest.slice(at + r.normRoot.length);
          const term = afterRoot.search(terminator);
          const relEnd = term < 0 ? afterRoot.length : term;
          const relRaw = afterRoot.slice(0, relEnd);
          const rel = relRaw.replace(/^\/+/, "");
          if (rel === "") {
            out += `analysis://${r.id}`;
          } else {
            const n = this.fileNumber(r.id, rel);
            out += `analysis://${r.id}/file-${this.pad(n)}.${extOf(rel)}`;
          }
          rest = afterRoot.slice(relEnd);
          continue outer;
        }
      }
      out += rest;
      break;
    }
    return out;
  }

  /**
   * SHAPE-redact a diagnostic message so a verbatim TS/Vue message (which can carry
   * identifiers, import paths, component names, string literals) never reaches an
   * artifact. Any known real root is stripped first; then the message is shaped in
   * a SINGLE left-to-right pass so no replacement is ever re-processed. The TS error
   * code (`TS####`) is kept (the stable discriminant), quoted spans collapse to
   * `'<id>'`, bare ASCII identifiers to `<id>`, numbers to `<n>`; punctuation and
   * whitespace are copied verbatim.
   */
  redactDiagnostic(message: string): string {
    const stripped = this.redactValue(message);
    const token = /(TS\d+)|('[^']*'|"[^"]*"|`[^`]*`)|([A-Za-z_$][A-Za-z0-9_$]*)|(\d+)/g;
    return stripped.replace(token, (_full, tsCode, quoted, _ident, num) => {
      if (tsCode) return tsCode as string; // keep the TS error code
      if (quoted) return "'<id>'"; // collapse a quoted span
      if (num) return "<n>"; // collapse a number
      return "<id>"; // collapse a bare identifier
    });
  }
}

/**
 * Serialize an already-redacted value to a pretty JSON string. The redaction MUST
 * have happened before this call — this wrapper does not redact, it only enforces
 * that emitters go through a typed, named "redacted writer" surface.
 */
export function serializeRedactedJson(redactedValue: unknown): string {
  return `${JSON.stringify(redactedValue, null, 2)}\n`;
}

/** Serialize an array of already-redacted records as JSONL. */
export function serializeRedactedJsonl(redactedRecords: readonly unknown[]): string {
  return (
    redactedRecords.map((r) => JSON.stringify(r)).join("\n") + (redactedRecords.length ? "\n" : "")
  );
}

/**
 * Redact a source map's leaky fields: rewrite every `sources` entry to an opaque
 * virtual id (an unknown-root source FAILS CLOSED → throws), and OMIT
 * `sourcesContent` entirely (external-corpus source bodies are never emitted).
 * Returns a new object; the input is not mutated.
 */
export function redactSourceMap(
  map: Record<string, unknown>,
  redactor: Redactor,
): Record<string, unknown> {
  const sources = Array.isArray(map.sources) ? map.sources : [];
  const redactedSources = sources.map((src) => {
    const opaque = typeof src === "string" ? redactor.sourceMapSource(src) : null;
    if (opaque === null) {
      // Fail closed: never emit a source we cannot redact to an opaque id.
      throw new Error("redactSourceMap: a `sources` entry is not under a known analysis root");
    }
    return opaque;
  });
  const out: Record<string, unknown> = { ...map, sources: redactedSources };
  // `sourcesContent` for corpus input is omitted entirely.
  delete out.sourcesContent;
  return out;
}
