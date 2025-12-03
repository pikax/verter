#!/usr/bin/env node
/*
  Generates:
  1. packages/types/src/tsx/tsx.attributes.ts - v-slot augmentations for HTML*Attributes
  2. packages/types/src/tsx/components-tsx.ts - ExtractFromHTMLElement type mapping
  
  by scanning @vue/runtime-dom's runtime-dom.d.ts for HTML*Attributes interfaces.
*/

const fs = require("fs");
const path = require("path");

function findWorkspaceRoot(startDir) {
  let dir = startDir;
  let nodeModulesRoot = null;
  while (true) {
    const nm = path.join(dir, "node_modules");
    const pnpm = path.join(dir, "pnpm-workspace.yaml");
    if (fs.existsSync(pnpm)) return dir; // explicit workspace root
    if (fs.existsSync(nm) && nodeModulesRoot === null) nodeModulesRoot = dir; // remember first up-tree node_modules

    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return nodeModulesRoot ?? startDir;
}

function resolveRuntimeDomDts(workspaceRoot) {
  try {
    const pkgPath = require.resolve("@vue/runtime-dom/package.json", {
      paths: [workspaceRoot],
    });
    const dir = path.dirname(pkgPath);
    const dts = path.join(dir, "dist", "runtime-dom.d.ts");
    if (fs.existsSync(dts)) return dts;
  } catch {}

  // pnpm fallback: scan .pnpm virtual store
  const storeDir = path.join(workspaceRoot, "node_modules", ".pnpm");
  if (fs.existsSync(storeDir)) {
    const entries = fs
      .readdirSync(storeDir)
      .filter((n) => n.startsWith("@vue+runtime-dom@"));
    // Prefer highest version by lexical order (pnpm stores exact versions)
    entries.sort().reverse();
    for (const ent of entries) {
      const dts = path.join(
        storeDir,
        ent,
        "node_modules",
        "@vue",
        "runtime-dom",
        "dist",
        "runtime-dom.d.ts"
      );
      if (fs.existsSync(dts)) return dts;
    }
  }

  throw new Error("Could not locate @vue/runtime-dom/dist/runtime-dom.d.ts");
}

function resolveLibDomDts(workspaceRoot) {
  try {
    return require.resolve("typescript/lib/lib.dom.d.ts", {
      paths: [workspaceRoot],
    });
  } catch {}
  // pnpm fallback
  const storeDir = path.join(workspaceRoot, "node_modules", ".pnpm");
  if (fs.existsSync(storeDir)) {
    const entries = fs
      .readdirSync(storeDir)
      .filter((n) => n.startsWith("typescript@"));
    entries.sort().reverse();
    for (const ent of entries) {
      const p = path.join(
        storeDir,
        ent,
        "node_modules",
        "typescript",
        "lib",
        "lib.dom.d.ts"
      );
      if (fs.existsSync(p)) return p;
    }
  }
  throw new Error("Could not locate typescript/lib/lib.dom.d.ts");
}

function buildAvailableDomElementSet(libDomPath) {
  const domSrc = fs.readFileSync(libDomPath, "utf8");
  const re = /interface\s+(HTML[A-Za-z0-9]+Element)\b/g;
  const set = new Set();
  let m;
  while ((m = re.exec(domSrc))) set.add(m[1]);
  return set;
}

function mapInterfaceToElement(ifaceName, elementsSet) {
  // Exact name match
  if (ifaceName === "HTMLAttributes") return "HTMLElement";
  if (ifaceName === "SVGAttributes") return null; // skip SVG here

  const base = ifaceName.replace(/HTMLAttributes$/, "");

  // Try straightforward convention first
  const direct = `HTML${base}Element`;
  if (elementsSet.has(direct)) return direct;

  // Heuristic variants for known casing peculiarities
  const variants = [""];
  // Textarea -> TextArea
  if (base.toLowerCase() === "textarea") variants.push("HTMLTextAreaElement");
  // Iframe -> IFrame
  if (base.toLowerCase() === "iframe") variants.push("HTMLIFrameElement");
  // Img -> Image
  if (base.toLowerCase() === "img") variants.push("HTMLImageElement");
  // Ol -> OList
  if (base === "Ol") variants.push("HTMLOListElement");
  // Li -> LI
  if (base === "Li") variants.push("HTMLLIElement");
  // Optgroup -> OptGroup
  if (base.toLowerCase() === "optgroup") variants.push("HTMLOptGroupElement");
  // Col/Colgroup -> TableCol
  if (base === "Col" || base.toLowerCase() === "colgroup")
    variants.push("HTMLTableColElement");
  // Td/Th -> TableCell
  if (base === "Td" || base === "Th") variants.push("HTMLTableCellElement");
  // Del/Ins -> Mod
  if (base === "Del" || base === "Ins") variants.push("HTMLModElement");
  // Blockquote -> Quote
  if (base.toLowerCase() === "blockquote") variants.push("HTMLQuoteElement");
  // Media -> Media
  if (base === "Media") variants.push("HTMLMediaElement");
  // Html -> Html
  if (base === "Html") variants.push("HTMLHtmlElement");

  for (const v of variants) if (elementsSet.has(v)) return v;

  // Fallback for legacy/not-present: Keygen, WebView, etc.
  return "HTMLElement";
}

function generate(packageRoot, workspaceRoot) {
  const dtsPath = resolveRuntimeDomDts(workspaceRoot);
  const src = fs.readFileSync(dtsPath, "utf8");
  const libDomPath = resolveLibDomDts(workspaceRoot);
  const elementSet = buildAvailableDomElementSet(libDomPath);

  const regex = /export\s+interface\s+(\w+HTMLAttributes)\b/g;
  const names = [];
  const seen = new Set();
  let m;
  while ((m = regex.exec(src))) {
    const name = m[1];
    if (seen.has(name)) continue;
    seen.add(name);
    names.push(name);
  }

  // Ensure generic HTMLAttributes is included (regex misses it)
  if (src.includes("interface HTMLAttributes") && !seen.has("HTMLAttributes")) {
    names.unshift("HTMLAttributes");
    seen.add("HTMLAttributes");
  }

  // Build augmentations
  const lines = [];
  lines.push(
    "/* This file is auto-generated by packages/types/scripts/generate-tsx-attributes.js */"
  );
  lines.push('import "vue/jsx";');
  lines.push('declare module "vue" {');
  names.forEach((iface) => {
    const elType = mapInterfaceToElement(iface, elementSet);
    if (!elType) return; // skip SVG
    lines.push(`  interface ${iface} {`);
    lines.push(`    "v-slot"?: (instance: ${elType}) => any;`);
    lines.push("  }");
  });
  lines.push("}");
  lines.push("");

  const outPath = path.join(packageRoot, "src", "tsx", "tsx.attributes.ts");
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, lines.join("\n"));
  return outPath;
}

/**
 * Build ordered element-to-attributes mappings for ExtractFromHTMLElement.
 * Order matters for TypeScript's structural typing - more specific types must come first.
 */
function buildOrderedMappings(names, elementSet) {
  const mappings = [];
  
  // Parent-child relationships: children must come before parents
  const parentChildRelations = {
    HTMLMediaElement: ["HTMLVideoElement", "HTMLAudioElement"],
  };
  
  // Types that should come late due to structural typing issues
  const lateOrderedTypes = new Set(["HTMLDataElement"]);
  
  const processedElements = new Set();
  const elementToAttr = new Map();
  
  // Build element -> attributes map
  for (const iface of names) {
    const elType = mapInterfaceToElement(iface, elementSet);
    if (!elType || elType === "HTMLElement") continue;
    elementToAttr.set(elType, iface);
  }
  
  // Process children before parents
  for (const [parent, children] of Object.entries(parentChildRelations)) {
    for (const child of children) {
      if (elementToAttr.has(child) && !processedElements.has(child)) {
        mappings.push({ element: child, attributes: elementToAttr.get(child) });
        processedElements.add(child);
      }
    }
    // Add parent after children
    if (elementToAttr.has(parent) && !processedElements.has(parent)) {
      mappings.push({ element: parent, attributes: elementToAttr.get(parent) });
      processedElements.add(parent);
    }
  }
  
  // Add remaining elements (except late-ordered ones)
  for (const [element, attr] of elementToAttr) {
    if (!processedElements.has(element) && !lateOrderedTypes.has(element)) {
      mappings.push({ element, attributes: attr });
      processedElements.add(element);
    }
  }
  
  // Add late-ordered types
  for (const element of lateOrderedTypes) {
    if (elementToAttr.has(element) && !processedElements.has(element)) {
      mappings.push({ element, attributes: elementToAttr.get(element) });
      processedElements.add(element);
    }
  }
  
  return mappings;
}

function generateComponentsTsx(packageRoot, names, elementSet) {
  const mappings = buildOrderedMappings(names, elementSet);
  
  const lines = [];
  lines.push("/* This file is auto-generated by packages/types/scripts/generate-tsx-attributes.cjs */");
  lines.push("");
  lines.push("/**");
  lines.push(" * Maps HTML element types to their corresponding Vue attribute interfaces.");
  lines.push(" * Used by ExtractComponentProps to provide proper attribute types for HTML elements.");
  lines.push(" *");
  lines.push(" */");
  lines.push("export type ExtractFromHTMLElement<T> =");
  
  for (const { element, attributes } of mappings) {
    lines.push(`  T extends ${element} ? import("vue").${attributes} :`);
  }
  
  // Fallback to HTMLAttributes
  lines.push(`  import("vue").HTMLAttributes;`);
  lines.push("");
  
  const outPath = path.join(packageRoot, "src", "tsx", "components-tsx.ts");
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, lines.join("\n"));
  return outPath;
}

if (require.main === module) {
  const packageRoot = path.resolve(__dirname, "..");
  const workspaceRoot = findWorkspaceRoot(packageRoot);
  
  // Generate tsx.attributes.ts
  const out = generate(packageRoot, workspaceRoot);
  console.log(`Generated ${path.relative(workspaceRoot, out)}`);
  
  // Generate components-tsx.ts
  const dtsPath = resolveRuntimeDomDts(workspaceRoot);
  const src = fs.readFileSync(dtsPath, "utf8");
  const libDomPath = resolveLibDomDts(workspaceRoot);
  const elementSet = buildAvailableDomElementSet(libDomPath);
  
  const regex = /export\s+interface\s+(\w+HTMLAttributes)\b/g;
  const names = [];
  const seen = new Set();
  let m;
  while ((m = regex.exec(src))) {
    const name = m[1];
    if (seen.has(name)) continue;
    seen.add(name);
    names.push(name);
  }
  if (src.includes("interface HTMLAttributes") && !seen.has("HTMLAttributes")) {
    names.unshift("HTMLAttributes");
  }
  
  const compOut = generateComponentsTsx(packageRoot, names, elementSet);
  console.log(`Generated ${path.relative(workspaceRoot, compOut)}`);
}

module.exports = {
  generate,
  generateComponentsTsx,
  findWorkspaceRoot,
  resolveRuntimeDomDts,
  resolveLibDomDts,
  buildAvailableDomElementSet,
  mapInterfaceToElement,
  buildOrderedMappings,
};
