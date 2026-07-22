/**
 * Authored-position probe mining for plain `.ts`/`.tsx` corpus files.
 *
 * The native counterpart of the SFC probe miner (`corpus-gate/probes.ts`):
 * pure text-shape scanning that selects PROBE POSITIONS, performing no
 * semantic resolution. Deterministic — same text, same probes. The categories
 * deliberately mirror the SFC miner's script-side classes (import names,
 * member-access completion points, function names for references) so the two
 * lanes probe the same POSITION CLASS: real authored identifiers an editor
 * user actually hovers, jumps from, completes after, and renames.
 */
import type { CorpusProbe } from "../corpus-gate/probes.js";

/** Per-category caps keep one pathological file from dominating the sample. */
const CATEGORY_CAPS: Readonly<Record<string, number>> = {
  importName: 4,
  memberCompl: 4,
  fnName: 3,
  typeName: 3,
  constName: 3,
};

const EOL = /\r\n|\n|\r/;

/** Mine authored probe positions from one plain-TS text (pure, deterministic). */
export function mineNativeTsProbes(text: string, maxProbes: number): CorpusProbe[] {
  const lines = text.split(EOL);
  const probes: CorpusProbe[] = [];
  const caps: Record<string, number> = {};
  const take = (category: string): boolean => {
    const cap = CATEGORY_CAPS[category] ?? 2;
    caps[category] = (caps[category] ?? 0) + 1;
    return caps[category] <= cap;
  };

  lines.forEach((line, i) => {
    if (probes.length >= maxProbes) return;
    // Skip lines with non-ASCII characters: keeps UTF-16 column arithmetic
    // trivially correct without encoding conversion on arbitrary corpora.
    if (/[^\x20-\x7E\t]/.test(line)) return;
    let m: RegExpExecArray | null;

    const importRe = /^\s*import\s+(?:type\s+)?\{?\s*([A-Za-z_$][\w$]*)/;
    if ((m = importRe.exec(line)) && m[1] !== "type") {
      const start = line.indexOf(m[1], line.indexOf("import"));
      if (take("importName"))
        probes.push({
          category: "importName",
          line: i,
          character: start,
          token: m[1],
          kinds: ["definition", "hover"],
        });
      return; // an import line contributes no other category
    }

    const typeRe = /^\s*(?:export\s+)?(?:interface|type|enum|class)\s+([A-Za-z_$][\w$]*)/;
    if ((m = typeRe.exec(line))) {
      const start = line.indexOf(m[1]);
      if (take("typeName"))
        probes.push({
          category: "typeName",
          line: i,
          character: start,
          token: m[1],
          kinds: ["definition", "hover"],
        });
    }

    const fnRe =
      /^\s*(?:export\s+)?(?:async\s+)?function\s+([a-zA-Z_$][\w$]*)|^\s*(?:export\s+)?const\s+([a-zA-Z_$][\w$]*)\s*=\s*(?:async\s*)?\(/;
    if ((m = fnRe.exec(line))) {
      const name = m[1] ?? m[2];
      const start = line.indexOf(name);
      if (take("fnName"))
        probes.push({
          category: "fnName",
          line: i,
          character: start,
          token: name,
          kinds: ["references", "hover"],
        });
    } else {
      const constRe = /^\s*(?:export\s+)?const\s+([a-zA-Z_$][\w$]*)\s*[:=]/;
      if ((m = constRe.exec(line))) {
        const start = line.indexOf(m[1]);
        if (take("constName"))
          probes.push({
            category: "constName",
            line: i,
            character: start,
            token: m[1],
            kinds: ["hover"],
          });
      }
    }

    const memberRe = /(?<![\w$.'"`])([a-zA-Z_$][\w$]*)\.([a-zA-Z_$][\w$]*)/g;
    while ((m = memberRe.exec(line))) {
      if (/["'/@.]/.test(line[m.index - 1] ?? "")) continue;
      const dotIndex = m.index + m[1].length;
      if (take("memberCompl")) {
        probes.push({
          category: "memberCompl",
          line: i,
          character: dotIndex + 1,
          token: `${m[1]}.${m[2]}`,
          kinds: ["completion", "hover"],
        });
      }
    }
  });
  return probes.slice(0, maxProbes);
}
