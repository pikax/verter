/**
 * Hermetic extractors for `.github/workflows/release.yml`.
 *
 * No YAML dependency is available in this package's resolution context, so the
 * few well-structured blocks these guards care about are parsed by
 * indentation, deterministically:
 *
 *   - a job's `strategy.matrix.include` rows (as key/value records), and
 *   - a job's raw body lines (for asserting the wiring steps exist).
 *
 * These are NOT general YAML parsers and are intentionally scoped. They fail
 * loudly — an empty result, which every consuming spec asserts against — when
 * the structure they expect is absent, so a workflow refactor that moved a
 * matrix cannot silently pass the reconciliation.
 */

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = dirname(dirname(fileURLToPath(import.meta.url)));

export const RELEASE_WORKFLOW_PATH = join(
  packageDir,
  "..",
  "..",
  ".github",
  "workflows",
  "release.yml",
);

/** One `strategy.matrix.include` row, as authored key/value strings. */
export type MatrixRow = Readonly<Record<string, string>>;

function indentOf(line: string): number {
  const m = /^(\s*)/.exec(line);
  return m ? m[1].length : 0;
}

function unquote(value: string): string {
  return value.trim().replace(/^["']|["']$/g, "");
}

export function readWorkflowText(workflowPath = RELEASE_WORKFLOW_PATH): string {
  return readFileSync(workflowPath, "utf8");
}

/**
 * The body lines of a job (everything more-indented than the job key).
 * Returns an empty array when the job key is absent.
 */
export function readJobBody(jobName: string, workflowPath = RELEASE_WORKFLOW_PATH): string[] {
  const lines = readWorkflowText(workflowPath).split(/\r?\n/);

  const keyPattern = new RegExp(`^(\\s*)${jobName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}:\\s*$`);
  let jobIdx = -1;
  let jobIndent = -1;
  for (let i = 0; i < lines.length; i++) {
    const m = keyPattern.exec(lines[i]);
    // A job key is a 2-space-indented key under top-level `jobs:`; anything
    // deeper is a step/`with:` key that happens to share the name.
    if (m && m[1].length === 2) {
      jobIdx = i;
      jobIndent = m[1].length;
      break;
    }
  }
  if (jobIdx === -1) return [];

  const body: string[] = [];
  for (let i = jobIdx + 1; i < lines.length; i++) {
    const line = lines[i];
    if (line.trim() === "") {
      body.push(line);
      continue;
    }
    if (indentOf(line) <= jobIndent) break;
    body.push(line);
  }
  return body;
}

/**
 * Extract a job's `strategy.matrix.include` rows. Order is preserved as
 * authored. Returns an empty array when the job or the include block is
 * absent.
 */
export function readMatrixRows(jobName: string, workflowPath = RELEASE_WORKFLOW_PATH): MatrixRow[] {
  const body = readJobBody(jobName, workflowPath);
  if (body.length === 0) return [];

  let includeIdx = -1;
  let includeIndent = -1;
  let sawMatrix = false;
  for (let i = 0; i < body.length; i++) {
    const line = body[i];
    if (line.trim() === "") continue;
    if (/^\s*matrix:\s*$/.test(line)) {
      sawMatrix = true;
      continue;
    }
    if (sawMatrix && /^\s*include:\s*$/.test(line)) {
      includeIdx = i;
      includeIndent = indentOf(line);
      break;
    }
  }
  if (includeIdx === -1) return [];

  const rows: Record<string, string>[] = [];
  let current: Record<string, string> | null = null;
  for (let i = includeIdx + 1; i < body.length; i++) {
    const line = body[i];
    if (line.trim() === "") continue;
    if (indentOf(line) <= includeIndent) break;

    const start = /^\s*-\s+([\w.-]+):\s*(.*)$/.exec(line);
    if (start) {
      current = { [start[1]]: unquote(start[2]) };
      rows.push(current);
      continue;
    }
    const cont = /^\s*([\w.-]+):\s*(.*)$/.exec(line);
    if (cont && current) current[cont[1]] = unquote(cont[2]);
  }

  return rows.map((row) => Object.freeze(row));
}
