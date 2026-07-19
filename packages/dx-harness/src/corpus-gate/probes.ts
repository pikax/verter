/**
 * Authored-position probe mining for corpus SFCs.
 *
 * Mines real, authored positions out of an SFC's text — component tags, prop
 * binds, event binds, interpolations, `v-for` aliases, slot names, class
 * tokens, member-access completion points, import names, macro variables —
 * and pairs each with the LSP request kinds that position should answer.
 * Pure text-shape scanning (this selects PROBE POSITIONS for a benchmark; it
 * performs no semantic resolution). Deterministic: same text, same probes.
 */
import type { CorpusRequestKind } from "./types.js";

export interface CorpusProbe {
  readonly category: string;
  /** 0-based line. */
  readonly line: number;
  /** UTF-16 column. */
  readonly character: number;
  readonly token: string;
  readonly kinds: readonly CorpusRequestKind[];
}

/** Per-category caps keep one pathological file from dominating the sample. */
const CATEGORY_CAPS: Readonly<Record<string, number>> = {
  componentTag: 3,
  propBind: 4,
  eventBind: 3,
  interp: 3,
  vfor: 2,
  slotName: 2,
  slotDef: 2,
  classToken: 3,
  templMemberCompl: 2,
  importName: 4,
  definePropsVar: 1,
  scriptMemberCompl: 3,
  scriptFn: 2,
};

const EOL = /\r\n|\n|\r/;

/** Mine authored probe positions from one SFC text (pure, deterministic). */
export function mineCorpusProbes(text: string, maxProbes: number): CorpusProbe[] {
  const lines = text.split(EOL);
  const probes: CorpusProbe[] = [];
  const templateOpen = lines.findIndex((line) => /^\s*<template/.test(line));
  let templateClose = -1;
  for (let i = lines.length - 1; i >= 0; i--) {
    if (/^\s*<\/template>/.test(lines[i])) {
      templateClose = i;
      break;
    }
  }
  const scriptOpen = lines.findIndex((line) => /<script/.test(line));
  let scriptClose = lines.findIndex((line, i) => i > scriptOpen && /<\/script>/.test(line));
  if (scriptClose === -1) scriptClose = lines.length;
  const hasStyle = /<style/.test(text);

  const caps: Record<string, number> = {};
  const take = (category: string): boolean => {
    const cap = CATEGORY_CAPS[category] ?? 2;
    caps[category] = (caps[category] ?? 0) + 1;
    return caps[category] <= cap;
  };
  const inTemplate = (i: number): boolean =>
    templateOpen !== -1 && i > templateOpen && (templateClose === -1 || i < templateClose);
  const inScript = (i: number): boolean => scriptOpen !== -1 && i > scriptOpen && i < scriptClose;

  lines.forEach((line, i) => {
    if (probes.length >= maxProbes) return;
    // Skip lines with non-ASCII characters: keeps UTF-16 column arithmetic
    // trivially correct without encoding conversion on arbitrary corpora.
    if (/[^\x20-\x7E\t]/.test(line)) return;
    let m: RegExpExecArray | null;
    if (inTemplate(i)) {
      const tagRe = /<([A-Z][A-Za-z0-9]*)/g;
      while ((m = tagRe.exec(line))) {
        if (take("componentTag"))
          probes.push({
            category: "componentTag",
            line: i,
            character: m.index + 1,
            token: m[1],
            kinds: ["hover", "definition"],
          });
      }
      const propRe = /(?<![\w@:.-])[:]([a-z][a-zA-Z0-9-]*)=/g;
      while ((m = propRe.exec(line))) {
        if (["key", "class", "style", "id"].includes(m[1])) continue;
        if (take("propBind"))
          probes.push({
            category: "propBind",
            line: i,
            character: m.index + 1,
            token: m[1],
            kinds: ["hover", "definition"],
          });
      }
      const eventRe = /@([a-z][a-zA-Z0-9-]*)=/g;
      while ((m = eventRe.exec(line))) {
        if (take("eventBind"))
          probes.push({
            category: "eventBind",
            line: i,
            character: m.index + 1,
            token: m[1],
            kinds: ["hover"],
          });
      }
      const interpRe = /\{\{\s*([a-zA-Z_$][\w$]*)/g;
      while ((m = interpRe.exec(line))) {
        const start = line.indexOf(m[1], m.index);
        if (take("interp"))
          probes.push({
            category: "interp",
            line: i,
            character: start,
            token: m[1],
            kinds: ["hover", "definition", "references"],
          });
      }
      const vforRe = /v-for="\(?\s*([a-zA-Z_$][\w$]*)/g;
      while ((m = vforRe.exec(line))) {
        const start = line.indexOf(m[1], m.index + 7);
        if (take("vfor"))
          probes.push({
            category: "vfor",
            line: i,
            character: start,
            token: m[1],
            kinds: ["hover"],
          });
      }
      const slotRe = /<template\s+#([a-zA-Z][\w-]*)/g;
      while ((m = slotRe.exec(line))) {
        const start = line.indexOf(`#${m[1]}`, m.index) + 1;
        if (take("slotName"))
          probes.push({
            category: "slotName",
            line: i,
            character: start,
            token: m[1],
            kinds: ["hover", "definition"],
          });
      }
      const slotDefRe = /<slot\s+name="([a-zA-Z][\w-]*)"/g;
      while ((m = slotDefRe.exec(line))) {
        const start = line.indexOf(m[1], m.index + 5);
        if (take("slotDef"))
          probes.push({
            category: "slotDef",
            line: i,
            character: start,
            token: m[1],
            kinds: ["hover"],
          });
      }
      if (hasStyle) {
        const classRe = /class="([a-z][\w-]*)/g;
        while ((m = classRe.exec(line))) {
          const start = line.indexOf(m[1], m.index);
          if (take("classToken"))
            probes.push({
              category: "classToken",
              line: i,
              character: start,
              token: m[1],
              kinds: ["hover", "definition"],
            });
        }
      }
      const memberRe = /([a-zA-Z_$][\w$]*)\.([a-zA-Z_$][\w$]*)/g;
      while ((m = memberRe.exec(line))) {
        if (/["'/@.]/.test(line[m.index - 1] ?? "")) continue;
        const dotIndex = m.index + m[1].length;
        if (take("templMemberCompl"))
          probes.push({
            category: "templMemberCompl",
            line: i,
            character: dotIndex + 1,
            token: `${m[1]}.${m[2]}`,
            kinds: ["completion", "hover"],
          });
      }
    } else if (inScript(i)) {
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
      }
      const definePropsRe =
        /(?:const|let)\s+([a-zA-Z_$][\w$]*)\s*=\s*(?:withDefaults\s*\(\s*)?defineProps/;
      if ((m = definePropsRe.exec(line))) {
        const start = line.indexOf(m[1]);
        if (take("definePropsVar"))
          probes.push({
            category: "definePropsVar",
            line: i,
            character: start,
            token: m[1],
            kinds: ["hover", "references"],
          });
      }
      const memberRe =
        /(?<![\w$.])(this|props|store|route|router|state|model|emit)\.([a-zA-Z_$][\w$]*)/g;
      while ((m = memberRe.exec(line))) {
        const dotIndex = m.index + m[1].length;
        if (take("scriptMemberCompl"))
          probes.push({
            category: "scriptMemberCompl",
            line: i,
            character: dotIndex + 1,
            token: `${m[1]}.${m[2]}`,
            kinds: ["completion", "hover"],
          });
      }
      const fnRe = /^\s{2,8}(?:async\s+)?([a-z][\w$]*)\s*\([^)]*\)\s*[:{]/;
      if (
        (m = fnRe.exec(line)) &&
        !["if", "for", "while", "switch", "catch", "constructor", "return"].includes(m[1])
      ) {
        const start = line.indexOf(m[1]);
        if (take("scriptFn"))
          probes.push({
            category: "scriptFn",
            line: i,
            character: start,
            token: m[1],
            kinds: ["references"],
          });
      }
    }
  });
  return probes.slice(0, maxProbes);
}
