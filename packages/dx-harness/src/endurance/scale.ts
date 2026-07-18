/**
 * Scale-lane support: derive content-checked probes from a corpus directory
 * (external via VERTER_ENDURANCE_CORPUS_DIR, or the synthetic generator
 * output). Usage is strictly READ-ONLY — files are read from disk and opened
 * as in-memory overlays; the harness never writes into the corpus (edits, in
 * the rename lane, are didChange overlays that are never saved).
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";

import type { EnduranceProbe } from "./session.js";
import { camelToKebab } from "./session.js";

export interface CorpusProbeDerivation {
  /** Corpus .vue files selected for opening (posix, corpus-relative). */
  readonly files: readonly string[];
  /** Content-checked probes derived from files[1..] (files[0] stays churn-only). */
  readonly probes: readonly EnduranceProbe[];
  /** File the storm/soak typer may churn (has a </script> tag), if any. */
  readonly churnFile: string | null;
  /** A `const` ident with a usage, suitable for rename cycles, if any. */
  readonly renameTarget: { file: string; ident: string } | null;
}

/** Recursively collect .vue files (skipping node_modules), sorted, capped. */
export function collectCorpusVueFiles(corpusDir: string, maxFiles: number): string[] {
  const found: string[] = [];
  const visit = (dir: string): void => {
    if (found.length >= maxFiles) return;
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    entries.sort((a, b) => a.name.localeCompare(b.name));
    for (const entry of entries) {
      if (found.length >= maxFiles) return;
      const absolute = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== "node_modules" && !entry.name.startsWith(".")) visit(absolute);
      } else if (entry.isFile() && entry.name.endsWith(".vue")) {
        found.push(path.relative(corpusDir, absolute).replaceAll("\\", "/"));
      }
    }
  };
  visit(corpusDir);
  return found;
}

const PROPS_USAGE = /props\.([A-Za-z_$][\w$]*)/g;
const INTERPOLATION = /\{\{\s*([A-Za-z_$][\w$]*)\s*\}\}/g;
const HANDLER_USAGE = /@click="([A-Za-z_$][\w$]*)"/g;
const CONST_DECL = /const\s+([A-Za-z_$][\w$]*)\s*=/g;
/** `<Tag` + `:prop=` sites — D1 attr-name completion ground (space position). */
const COMPONENT_ATTR = /(<[A-Z][\w$]*) :([A-Za-z_$][\w$]*)=/g;

function unique(values: string[]): string[] {
  return [...new Set(values)];
}

/**
 * Derive probes from corpus content. Best-effort and honest: only patterns
 * that make the expectation deterministic are used (a `props.<ident>` usage ⇒
 * hover contains the ident; `@click="fn"` with a same-file `function fn(` ⇒
 * definition lands on that line).
 */
export function deriveCorpusProbes(
  corpusDir: string,
  options: { maxFiles: number },
): CorpusProbeDerivation {
  const root = path.resolve(corpusDir);
  if (!statSync(root, { throwIfNoEntry: false })?.isDirectory()) {
    throw new Error(`corpus directory does not exist: ${root}`);
  }
  const files = collectCorpusVueFiles(root, options.maxFiles);
  const probes: EnduranceProbe[] = [];
  let churnFile: string | null = null;
  let renameTarget: { file: string; ident: string } | null = null;

  files.forEach((relativePath, index) => {
    const text = readFileSync(path.join(root, relativePath), "utf8");

    if (index === 0) {
      if (text.includes("</script>")) churnFile = relativePath;
      for (const match of text.matchAll(CONST_DECL)) {
        const ident = match[1];
        const escaped = ident.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        const occurrences = text.match(new RegExp(`\\b${escaped}\\b`, "g"))?.length ?? 0;
        if (occurrences >= 2) {
          renameTarget = { file: relativePath, ident };
          break;
        }
      }
      return; // files[0] carries no probes — it is the churn/rename target.
    }

    const propsIdents = unique([...text.matchAll(PROPS_USAGE)].map((m) => m[1]));
    for (const ident of propsIdents.slice(0, 2)) {
      probes.push({
        // Provider member hover: documented type-quality gap — informational.
        kind: "hover",
        relativePath,
        needle: `props.${ident}`,
        cursorOffset: "props.".length,
        expectIncludes: [],
        informational: true,
        label: `${relativePath} props.${ident} hover`,
      });
    }
    if (propsIdents.length > 0) {
      probes.push({
        // Script member completion: documented type-quality gap — informational.
        kind: "completion",
        relativePath,
        needle: "props.",
        cursorOffset: "props.".length,
        expectLabels: propsIdents.slice(0, 2),
        informational: true,
        label: `${relativePath} props. member completion`,
      });
    }
    const interpolation = [...text.matchAll(INTERPOLATION)][0];
    if (interpolation) {
      probes.push({
        // Vue binding hover on a local: Verter owns the answer — strong.
        // Cursor INSIDE the identifier (offset 2 lands on the space before it).
        kind: "hover",
        relativePath,
        needle: interpolation[0],
        cursorOffset: interpolation[0].indexOf(interpolation[1]) + 1,
        expectIncludes: [interpolation[1]],
        requireNonEmpty: true,
        label: `${relativePath} interpolation hover`,
      });
    }
    for (const match of text.matchAll(HANDLER_USAGE)) {
      const handler = match[1];
      if (text.includes(`function ${handler}(`)) {
        probes.push({
          kind: "definition",
          relativePath,
          needle: `@click="${handler}"`,
          cursorOffset: 8,
          expectLineNeedle: `function ${handler}`,
          label: `${relativePath} handler definition`,
        });
        break;
      }
    }
    // D1 component attr-name completion on child component tags (strong).
    // Probes the SPACE after the tag name (the fresh attr-name position) —
    // mid-token after a colon of an existing attr is not fresh-bind ground.
    for (const match of text.matchAll(COMPONENT_ATTR)) {
      const needle = `${match[1]} `;
      probes.push({
        kind: "completion",
        relativePath,
        needle,
        cursorOffset: needle.length,
        expectLabels: [camelToKebab(match[2])],
        label: `${relativePath} component attr completion (D1)`,
      });
      break; // one per file is enough load
    }
  });

  return { files, probes, churnFile, renameTarget };
}
