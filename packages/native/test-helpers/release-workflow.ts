/**
 * Hermetic extractor for the `build-native` job's target matrix in
 * `.github/workflows/release.yml`. No YAML dependency is available in this
 * package's resolution context, so we parse the single, well-structured
 * block we care about — `jobs.build-native.strategy.matrix.include[].target`
 * — by indentation, deterministically.
 *
 * This is NOT a general YAML parser and is intentionally scoped: it locates
 * the `build-native:` job key, descends to its `matrix:` → `include:`
 * sequence, and collects every `- target: <value>` entry until the block
 * dedents. It fails loudly (returns an empty list, which the reconciliation
 * spec asserts against) if the structure it expects is absent, so a
 * refactor of the workflow that moved the matrix can't silently pass.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { PACKAGE_DIR } from "../platforms.ts";

export const RELEASE_WORKFLOW_PATH = join(
  PACKAGE_DIR,
  "..",
  "..",
  ".github",
  "workflows",
  "release.yml",
);

function indentOf(line: string): number {
  const m = /^(\s*)/.exec(line);
  return m ? m[1].length : 0;
}

/**
 * Extract the rust-target list from the `build-native` job's
 * `strategy.matrix.include`. Order is preserved as authored.
 */
export function readBuildNativeTargets(workflowPath = RELEASE_WORKFLOW_PATH): string[] {
  const text = readFileSync(workflowPath, "utf8");
  const lines = text.split(/\r?\n/);

  // 1) Find the `build-native:` job key (a job is a 2-space-indented key
  //    under top-level `jobs:`). We match the key precisely.
  let jobIdx = -1;
  let jobIndent = -1;
  for (let i = 0; i < lines.length; i++) {
    const m = /^(\s*)build-native:\s*$/.exec(lines[i]);
    if (m) {
      jobIdx = i;
      jobIndent = m[1].length;
      break;
    }
  }
  if (jobIdx === -1) return [];

  // 2) Within the job body (more-indented than the job key), find the
  //    `include:` key that sits under `matrix:`. We scan to the end of the
  //    job body.
  let includeIdx = -1;
  let includeIndent = -1;
  let sawMatrix = false;
  for (let i = jobIdx + 1; i < lines.length; i++) {
    const line = lines[i];
    if (line.trim() === "") continue;
    const ind = indentOf(line);
    // Left the job body.
    if (ind <= jobIndent) break;
    if (/^\s*matrix:\s*$/.test(line)) {
      sawMatrix = true;
      continue;
    }
    if (sawMatrix && /^\s*include:\s*$/.test(line)) {
      includeIdx = i;
      includeIndent = ind;
      break;
    }
  }
  if (includeIdx === -1) return [];

  // 3) Collect every `- target: <value>` in the include sequence until the
  //    block dedents to or past the `include:` indent.
  const targets: string[] = [];
  for (let i = includeIdx + 1; i < lines.length; i++) {
    const line = lines[i];
    if (line.trim() === "") continue;
    const ind = indentOf(line);
    if (ind <= includeIndent) break;
    const m = /^\s*-?\s*target:\s*(\S+)\s*$/.exec(line);
    if (m) targets.push(m[1].replace(/^["']|["']$/g, ""));
  }
  return targets;
}
