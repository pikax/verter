/**
 * Path and position helpers shared across the harness scaffold.
 *
 * `canonicalizePath` mirrors `verter_span::path::canonicalize_path` (the Rust
 * canonical-id normaliser the `verter_dx_baseline` bridge applies on its side)
 * so a path B computes (`workspaceRoot`, `expectedTsserverJs`, …) compares equal
 * to the value C derives from it: forward slashes only, the Windows
 * extended-length prefix stripped, a lowercase drive letter, and no trailing
 * slash except a filesystem root. Building paths with this helper — rather than
 * string concatenation with a hardcoded separator — keeps the harness portable
 * across macOS, Windows, and Linux.
 */

import { posix } from "node:path";

/** A leading `[A-Z]:` drive letter that must be lowercased. */
const UPPER_DRIVE = /^([A-Z]):/;

/**
 * Whether `s` ends with a trailing `/` that must be stripped — i.e. it is not the
 * filesystem root `/` and not a Windows drive-root `x:/`. Mirrors
 * `verter_span::path::ends_with_strippable_slash`.
 */
function endsWithStrippableSlash(s: string): boolean {
  if (!s.endsWith("/") || s === "/") return false;
  // drive-root `x:/`
  return !(s.length === 3 && s[1] === ":" && s[2] === "/");
}

/**
 * Normalise a raw filesystem path to the canonical form the Rust side uses.
 *
 * In order: backslash → forward slash; strip `\\?\UNC\` (→ `//`) and `\\?\`
 * prefixes; lowercase a leading Windows drive letter (keeping the colon); strip
 * EVERY trailing slash unless the result is a root (`/` or `x:/`).
 */
export function canonicalizePath(raw: string): string {
  let p = raw.replace(/\\/g, "/");

  if (p.startsWith("//?/UNC/")) {
    p = "//" + p.slice("//?/UNC/".length);
  } else if (p.startsWith("//?/")) {
    p = p.slice("//?/".length);
  }

  p = p.replace(UPPER_DRIVE, (_m, d: string) => `${d.toLowerCase()}:`);

  // Strip ALL trailing slashes except the roots `/` and `x:/`. Looped so the
  // result is idempotent — `/a//` and `/a///` both canonicalise to `/a`,
  // mirroring `verter_span::path::canonicalize_path`; a single strip would leave
  // a residual `/a/` and yield two canonical ids for the same directory.
  while (endsWithStrippableSlash(p)) {
    p = p.slice(0, -1);
  }

  return p;
}

/**
 * Join path segments onto a CANONICAL base, preserving a leading `//` (UNC)
 * prefix that `path.posix.join` collapses to a single slash.
 *
 * `canonicalizePath` renders a Windows UNC path (`\\?\UNC\server\share` or
 * `\\server\share`) as `//server/share`, mirroring `verter_span`. But
 * `path.posix.join("//server/share", "node_modules")` normalises the leading
 * `//` down to `/server/share/node_modules`, which would yield a second,
 * non-canonical id for the same location and diverge from the value C derives.
 * This helper joins exactly like `posix.join` and then re-attaches the leading
 * slash whenever the base was UNC and the join collapsed it. A non-UNC base
 * (drive-letter or plain POSIX) is byte-for-byte identical to `posix.join`.
 */
export function joinCanonical(base: string, ...segments: string[]): string {
  const joined = posix.join(base, ...segments);
  // A UNC base starts with exactly `//`; re-attach the slash `posix.join` ate.
  if (base.startsWith("//") && !joined.startsWith("//")) {
    return `/${joined}`;
  }
  return joined;
}

/** A 0-based LSP-style position: line plus a UTF-16 code-unit column. */
export interface LineChar {
  line: number;
  character: number;
}

/**
 * Convert a JS string index (a UTF-16 code-unit offset) into a 0-based
 * `{ line, character }` position, the unit LSP uses for document positions.
 *
 * Line breaks are folded the way LSP folds them — `\r\n`, lone `\r`, and `\n`
 * each advance the line by one — so the same logical position in a CRLF file and
 * its LF twin yields the same `{ line, character }`. The column is a count of
 * UTF-16 code units from the start of the line (a surrogate-pair character
 * counts as two). `index` is clamped to `[0, text.length]`.
 */
export function offsetToLineChar(text: string, index: number): LineChar {
  const target = Math.min(Math.max(index, 0), text.length);
  let line = 0;
  let lineStart = 0;
  let i = 0;
  while (i < target) {
    const code = text.charCodeAt(i);
    if (code === 13 /* \r */) {
      line += 1;
      // A `\r\n` pair is a single break; advance past the `\n` too.
      i += i + 1 < text.length && text.charCodeAt(i + 1) === 10 ? 2 : 1;
      lineStart = i;
    } else if (code === 10 /* \n */) {
      line += 1;
      i += 1;
      lineStart = i;
    } else {
      i += 1;
    }
  }
  // `lineStart` can momentarily exceed `target` only if `target` split a CRLF
  // pair, which anchor positions never do; guard the column to stay non-negative.
  return { line, character: Math.max(0, target - lineStart) };
}
