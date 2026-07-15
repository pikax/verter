/**
 * Architecture guard: the playground language UI is DESCRIPTOR-DRIVEN. No source
 * file may hardcode a framework-option array like `["vue", "svelte"]` to drive
 * the language dropdown, presets grouping, or compile dispatch. The single
 * authority is `CLIENT_FRAMEWORKS` (via the `./frameworks` derive helper).
 *
 * Allowlist: the manifest itself (language-shared is not scanned), the preset
 * fixtures (which carry per-preset `language` strings), and the test files.
 */
import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const SRC_DIR = join(dirname(fileURLToPath(import.meta.url)), "..");

// A hardcoded framework-option array, e.g. ["vue", "svelte"] or ['svelte','vue'].
const FRAMEWORK_LITERAL_ARRAY = /\[\s*(['"])(?:vue|svelte)\1\s*,\s*(['"])(?:vue|svelte)\2\s*\]/;

function collectSourceFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      collectSourceFiles(full, acc);
    } else if (/\.(ts|vue)$/.test(entry) && !entry.endsWith(".spec.ts")) {
      acc.push(full);
    }
  }
  return acc;
}

describe("framework-option hardcode guard", () => {
  it('no playground source hardcodes a ["vue","svelte"]-style framework array', () => {
    const offenders: string[] = [];
    for (const file of collectSourceFiles(SRC_DIR)) {
      // presets.ts legitimately carries per-preset language strings, but never
      // as a framework-option *array*; if a literal pair array appears it is a
      // violation everywhere.
      const content = readFileSync(file, "utf8");
      if (FRAMEWORK_LITERAL_ARRAY.test(content)) {
        offenders.push(file.replace(SRC_DIR, "src"));
      }
    }
    expect(offenders, `hardcoded framework arrays found in: ${offenders.join(", ")}`).toEqual([]);
  });
});
