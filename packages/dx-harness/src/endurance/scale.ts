/** Read-only scale-corpus discovery and deterministic probe derivation. */
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";

import type { EnduranceProbe } from "./session.js";
import { camelToKebab } from "./session.js";
import type { EnduranceFramework, EnduranceLanguageMode } from "./types.js";

export interface CorpusLaneSection {
  readonly framework: EnduranceFramework;
  readonly mode: EnduranceLanguageMode;
  readonly files: readonly string[];
  readonly probes: readonly EnduranceProbe[];
  readonly renameTarget: { readonly file: string; readonly ident: string } | null;
}

export interface CorpusProbeDerivation {
  readonly files: readonly string[];
  readonly probes: readonly EnduranceProbe[];
  readonly churnFile: string | null;
  readonly renameTarget: { file: string; ident: string } | null;
  readonly lanes: readonly CorpusLaneSection[];
}

function laneOf(
  relativePath: string,
  text: string,
): {
  framework: EnduranceFramework;
  mode: EnduranceLanguageMode;
} {
  return {
    framework: relativePath.endsWith(".svelte") ? "svelte" : "vue",
    mode: /<script[^>]*\blang=["']ts["']/.test(text) ? "ts" : "js",
  };
}

/** Recursively collect `.vue` and `.svelte` files, fairly capped across all four lanes. */
export function collectCorpusCarrierFiles(corpusDir: string, maxFiles: number): string[] {
  const found: string[] = [];
  const visit = (dir: string): void => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    entries.sort((a, b) => a.name.localeCompare(b.name));
    for (const entry of entries) {
      const absolute = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== "node_modules" && !entry.name.startsWith(".")) visit(absolute);
      } else if (entry.isFile() && /\.(?:vue|svelte)$/.test(entry.name)) {
        found.push(path.relative(corpusDir, absolute).replaceAll("\\", "/"));
      }
    }
  };
  visit(corpusDir);
  const buckets = new Map<string, string[]>();
  for (const relativePath of found.sort()) {
    const text = readFileSync(path.join(corpusDir, relativePath), "utf8");
    const lane = laneOf(relativePath, text);
    const key = `${lane.framework}-${lane.mode}`;
    const bucket = buckets.get(key) ?? [];
    bucket.push(relativePath);
    buckets.set(key, bucket);
  }
  const orderedKeys = ["vue-ts", "vue-js", "svelte-ts", "svelte-js"];
  const selected: string[] = [];
  for (let index = 0; selected.length < maxFiles; index += 1) {
    let added = false;
    for (const key of orderedKeys) {
      const file = buckets.get(key)?.[index];
      if (file === undefined) continue;
      selected.push(file);
      added = true;
      if (selected.length >= maxFiles) break;
    }
    if (!added) break;
  }
  return selected;
}

const PROPS_USAGE = /props\.([A-Za-z_$][\w$]*)/g;
const VUE_INTERPOLATION = /\{\{\s*([A-Za-z_$][\w$]*)\s*\}\}/g;
const SVELTE_INTERPOLATION = /^\s*<[^>]+>\{([A-Za-z_$][\w$]*)\}<\/[^>]+>\s*$/gm;
const VUE_HANDLER = /@click="([A-Za-z_$][\w$]*)"/g;
const SVELTE_HANDLER = /onclick=\{([A-Za-z_$][\w$]*)\}/g;
const SVELTE_CALLBACK = /\b(onselect)\?\.\(/g;
const SVELTE_SNIPPET_DECL = /\{#snippet\s+([A-Za-z_$][\w$]*)\(/g;
const SVELTE_SNIPPET_RENDER = /@render\s+([A-Za-z_$][\w$]*)\(/g;
const SCRIPT_LOCAL = /const\s+[A-Za-z_$][\w$]*Length\s*=\s*([A-Za-z_$][\w$]*)\.length/g;
const CONST_DECL = /(?:const|let)\s+([A-Za-z_$][\w$]*)\s*=/g;
const VUE_COMPONENT_ATTR = /(<[A-Z][\w$]*) :([A-Za-z_$][\w$]*)=/g;
const SVELTE_COMPONENT_ATTR = /(<[A-Z][\w$]*) ([A-Za-z_$][\w$]*)=/g;

function unique(values: string[]): string[] {
  return [...new Set(values)];
}

function resolveImportedComponent(
  root: string,
  importer: string,
  text: string,
  tag: string,
  framework: EnduranceFramework,
): { relativePath: string; source: string } | null {
  const escapedTag = tag.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const importMatch = new RegExp(`import\\s+${escapedTag}\\s+from\\s+["']([^"']+)["']`).exec(text);
  const importSource = importMatch?.[1];
  if (!importSource?.startsWith(".")) return null;

  const unresolved = path.resolve(root, path.dirname(importer), importSource);
  const candidates = path.extname(unresolved)
    ? [unresolved]
    : [`${unresolved}.${framework}`, path.join(unresolved, `index.${framework}`)];
  for (const candidate of candidates) {
    const relative = path.relative(root, candidate);
    if (relative.startsWith("..") || path.isAbsolute(relative)) continue;
    if (!statSync(candidate, { throwIfNoEntry: false })?.isFile()) continue;
    try {
      return {
        relativePath: relative.replaceAll("\\", "/"),
        source: readFileSync(candidate, "utf8"),
      };
    } catch {
      return null;
    }
  }
  return null;
}

export function deriveCorpusProbes(
  corpusDir: string,
  options: { maxFiles: number },
): CorpusProbeDerivation {
  const root = path.resolve(corpusDir);
  if (!statSync(root, { throwIfNoEntry: false })?.isDirectory()) {
    throw new Error(`corpus directory does not exist: ${root}`);
  }
  const files = collectCorpusCarrierFiles(root, options.maxFiles);
  const probes: EnduranceProbe[] = [];
  const sections = new Map<
    string,
    {
      framework: EnduranceFramework;
      mode: EnduranceLanguageMode;
      files: string[];
      probes: EnduranceProbe[];
      renameTarget: { file: string; ident: string } | null;
    }
  >();
  let churnFile: string | null = null;
  let renameTarget: { file: string; ident: string } | null = null;

  files.forEach((relativePath) => {
    const text = readFileSync(path.join(root, relativePath), "utf8");
    const lane = laneOf(relativePath, text);
    const key = `${lane.framework}-${lane.mode}`;
    const section = sections.get(key) ?? { ...lane, files: [], probes: [], renameTarget: null };
    section.files.push(relativePath);
    sections.set(key, section);

    if (churnFile === null && text.includes("</script>")) churnFile = relativePath;
    if (renameTarget === null || section.renameTarget === null) {
      for (const match of text.matchAll(CONST_DECL)) {
        const ident = match[1];
        const escaped = ident.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        if ((text.match(new RegExp(`\\b${escaped}\\b`, "g"))?.length ?? 0) >= 2) {
          const target = { file: relativePath, ident };
          if (renameTarget === null) renameTarget = target;
          if (section.renameTarget === null) section.renameTarget = target;
          break;
        }
      }
    }

    const addProbe = (probe: EnduranceProbe): void => {
      probes.push(probe);
      section.probes.push(probe);
    };
    const propsIdents = unique([...text.matchAll(PROPS_USAGE)].map((match) => match[1]));
    for (const ident of propsIdents.slice(0, 2)) {
      addProbe({
        kind: "hover",
        relativePath,
        needle: `props.${ident}`,
        cursorOffset: "props.".length,
        expectIncludes: [],
        informational: true,
        label: `${key} ${relativePath} props.${ident} hover`,
      });
    }
    if (propsIdents.length > 0) {
      addProbe({
        kind: "completion",
        relativePath,
        needle: "props.",
        cursorOffset: "props.".length,
        expectLabels: propsIdents.slice(0, 2),
        informational: true,
        label: `${key} ${relativePath} props member completion`,
      });
    }

    const interpolationRegex = lane.framework === "vue" ? VUE_INTERPOLATION : SVELTE_INTERPOLATION;
    const interpolation = [...text.matchAll(interpolationRegex)][0];
    if (interpolation) {
      const scriptLocal = [...text.matchAll(SCRIPT_LOCAL)].at(-1);
      const local = scriptLocal?.[1] ?? interpolation[1];
      const needle = scriptLocal ? `${local}.length` : interpolation[0].trim();
      addProbe({
        kind: "hover",
        relativePath,
        needle,
        cursorOffset: scriptLocal ? 2 : needle.indexOf(local) + 1,
        expectIncludes: [local],
        forbidIncludes: ["any"],
        requireNonEmpty: true,
        label: `${key} ${relativePath} interpolation hover`,
      });
    }

    const handlerRegex = lane.framework === "vue" ? VUE_HANDLER : SVELTE_HANDLER;
    for (const match of text.matchAll(handlerRegex)) {
      const handler = match[1];
      if (text.includes(`function ${handler}(`)) {
        addProbe({
          kind: "definition",
          relativePath,
          needle: match[0],
          cursorOffset: match[0].indexOf(handler) + 1,
          expectLineNeedle: `function ${handler}`,
          label: `${key} ${relativePath} handler definition`,
        });
        break;
      }
    }

    if (lane.framework === "svelte") {
      const callback = [...text.matchAll(SVELTE_CALLBACK)][0];
      if (callback) {
        addProbe({
          kind: "definition",
          relativePath,
          needle: callback[0],
          cursorOffset: 2,
          expectLineNeedle: "onselect, content }",
          label: `${key} ${relativePath} callback-event definition`,
        });
      }
      const snippetDeclaration = [...text.matchAll(SVELTE_SNIPPET_DECL)][0];
      const snippetRender = [...text.matchAll(SVELTE_SNIPPET_RENDER)].find(
        (match) => match[1] === snippetDeclaration?.[1],
      );
      if (snippetDeclaration && snippetRender) {
        addProbe({
          kind: "definition",
          relativePath,
          needle: snippetRender[0],
          cursorOffset: "@render ".length + 1,
          expectLineNeedle: snippetDeclaration[0],
          label: `${key} ${relativePath} snippet definition`,
        });
      }
      if (lane.mode === "ts" && text.includes("const contentRef = content")) {
        addProbe({
          kind: "hover",
          relativePath,
          needle: "Snippet<[string]>",
          cursorOffset: 2,
          expectIncludes: ["Snippet"],
          forbidIncludes: ["any"],
          requireNonEmpty: true,
          label: `${key} ${relativePath} snippet hover`,
        });
      }
    }

    const componentRegex = lane.framework === "vue" ? VUE_COMPONENT_ATTR : SVELTE_COMPONENT_ATTR;
    for (const match of text.matchAll(componentRegex)) {
      const bareNeedle = `${match[1]} />`;
      const needle = text.includes(bareNeedle) ? bareNeedle : `${match[1]} `;
      const tag = match[1].slice(1);
      const target = resolveImportedComponent(root, relativePath, text, tag, lane.framework);
      if (!target) break;
      const unusedProp = /\b(unusedonly\d{6})\??\s*:/.exec(target.source)?.[1];
      if (!unusedProp) break;
      addProbe({
        kind: "completion",
        relativePath,
        needle,
        cursorOffset: needle.endsWith("/>") ? needle.length - 2 : needle.length,
        expectLabels: [lane.framework === "vue" ? camelToKebab(unusedProp) : unusedProp],
        label: `${key} ${relativePath} component attr completion`,
      });
      if (lane.framework === "svelte") {
        addProbe({
          kind: "definition",
          relativePath,
          needle: match[1],
          cursorOffset: 2,
          expectUriSuffix: `/${target.relativePath}`,
          label: `${key} ${relativePath} component definition`,
        });
      }
      break;
    }
  });

  return {
    files,
    probes,
    churnFile,
    renameTarget,
    lanes: ["vue-ts", "vue-js", "svelte-ts", "svelte-js"]
      .map((key) => sections.get(key))
      .filter((section): section is NonNullable<typeof section> => section !== undefined),
  };
}
