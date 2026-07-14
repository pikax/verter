#!/usr/bin/env node
/*
  Generates crates/verter_analysis/src/html_intrinsics_data.rs from
  @vue/runtime-dom/dist/runtime-dom.d.ts.

  This is the built-in fallback catalog only. Runtime project loading may
  inject a project-local intrinsic catalog derived from the consumer project's
  installed TypeScript/Vue JSX surface.

  The generated file contains deterministic raw intrinsic member data only.
  crates/verter_analysis/src/html_intrinsics.rs remains the thin runtime
  wrapper that maps the raw catalog into public Rust types.
*/

const fs = require("fs");
const path = require("path");

let ts;

function resolveRuntimeDomDts(root) {
  try {
    const pkgPath = require.resolve("@vue/runtime-dom/package.json", { paths: [root] });
    const dir = path.dirname(pkgPath);
    const dts = path.join(dir, "dist", "runtime-dom.d.ts");
    if (fs.existsSync(dts)) return dts;
  } catch {}

  const storeDir = path.join(root, "node_modules", ".pnpm");
  if (fs.existsSync(storeDir)) {
    const entries = fs.readdirSync(storeDir).filter((name) => name.startsWith("@vue+runtime-dom@"));
    entries.sort().reverse();
    for (const entry of entries) {
      const dts = path.join(
        storeDir,
        entry,
        "node_modules",
        "@vue",
        "runtime-dom",
        "dist",
        "runtime-dom.d.ts",
      );
      if (fs.existsSync(dts)) return dts;
    }
  }

  throw new Error("Could not locate @vue/runtime-dom/dist/runtime-dom.d.ts");
}

function loadTypeScript(root) {
  try {
    const tsEntry = require.resolve("typescript", { paths: [root] });
    return require(tsEntry);
  } catch {}

  const storeDir = path.join(root, "node_modules", ".pnpm");
  if (fs.existsSync(storeDir)) {
    const entries = fs.readdirSync(storeDir).filter((name) => name.startsWith("typescript@"));
    entries.sort().reverse();
    for (const entry of entries) {
      const tsEntry = path.join(
        storeDir,
        entry,
        "node_modules",
        "typescript",
        "lib",
        "typescript.js",
      );
      if (fs.existsSync(tsEntry)) {
        return require(tsEntry);
      }
    }
  }

  throw new Error("Could not locate the TypeScript runtime");
}

function resolveRuntimeDomPackage(root) {
  try {
    const pkgPath = require.resolve("@vue/runtime-dom/package.json", { paths: [root] });
    return JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  } catch {
    return null;
  }
}

function stableCompare(left, right) {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function constName(name) {
  return name
    .replace(/HTMLAttributes$/, "_HTML_ATTRIBUTES")
    .replace(/SVGAttributes$/, "_SVG_ATTRIBUTES")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1_$2")
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/_+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toUpperCase();
}

function onPropToEventName(name) {
  return /^on[A-Z]/.test(name) ? name[2].toLowerCase() + name.slice(3) : null;
}

function cleanTypeText(typeText) {
  return typeText
    .replace(/\s+/g, " ")
    .replace(/\s*\|\s*undefined\b/g, "")
    .trim();
}

function propertyNameText(nameNode, sourceFile) {
  if (ts.isIdentifier(nameNode) || ts.isStringLiteral(nameNode) || ts.isNumericLiteral(nameNode)) {
    return nameNode.text;
  }
  return nameNode.getText(sourceFile);
}

function collectInterfaces(sourceFile) {
  const interfaces = new Map();

  sourceFile.forEachChild((node) => {
    if (!ts.isInterfaceDeclaration(node)) return;

    const heritage = [];
    for (const clause of node.heritageClauses ?? []) {
      if (clause.token !== ts.SyntaxKind.ExtendsKeyword) continue;
      for (const heritageType of clause.types) {
        heritage.push({
          name: heritageType.expression.getText(sourceFile),
          typeArgs: (heritageType.typeArguments ?? []).map((arg) => arg.getText(sourceFile)),
        });
      }
    }

    const members = [];
    for (const member of node.members) {
      if (!ts.isPropertySignature(member) || !member.name) continue;
      const name = propertyNameText(member.name, sourceFile);
      if (!name || name.startsWith("[")) continue;
      const eventName = onPropToEventName(name);
      members.push({
        name: eventName ?? name,
        kind: eventName ? "Listener" : "Attr",
        rawType: cleanTypeText(member.type ? member.type.getText(sourceFile) : "any"),
      });
    }

    interfaces.set(node.name.text, {
      heritage,
      members,
    });
  });

  return interfaces;
}

function mergeMembers(base, incoming) {
  const map = new Map();
  for (const member of base) {
    map.set(`${member.kind}:${member.name}`, member);
  }
  for (const member of incoming) {
    map.set(`${member.kind}:${member.name}`, member);
  }
  return Array.from(map.values()).sort((left, right) => {
    const kindOrder = stableCompare(left.kind, right.kind);
    return kindOrder !== 0 ? kindOrder : stableCompare(left.name, right.name);
  });
}

function buildResolvedInterfaces(interfaces) {
  const cache = new Map();
  const visiting = new Set();

  function resolveInterface(name) {
    if (cache.has(name)) return cache.get(name);
    if (visiting.has(name)) {
      throw new Error(`Interface inheritance cycle detected at ${name}`);
    }

    visiting.add(name);
    const info = interfaces.get(name);
    if (!info) {
      visiting.delete(name);
      cache.set(name, []);
      return [];
    }

    let members = [];
    for (const heritage of info.heritage) {
      if (heritage.name === "EventHandlers" && heritage.typeArgs[0] === "Events") {
        members = mergeMembers(members, resolveInterface("Events"));
        continue;
      }
      members = mergeMembers(members, resolveInterface(heritage.name));
    }
    members = mergeMembers(members, info.members);

    visiting.delete(name);
    cache.set(name, members);
    return members;
  }

  for (const name of interfaces.keys()) {
    resolveInterface(name);
  }

  return cache;
}

function collectTagInterfaces(sourceFile) {
  const mapping = new Map();

  sourceFile.forEachChild((node) => {
    if (!ts.isInterfaceDeclaration(node) || node.name.text !== "IntrinsicElementAttributes") {
      return;
    }

    for (const member of node.members) {
      if (!ts.isPropertySignature(member) || !member.type || !member.name) continue;
      const tagName = propertyNameText(member.name, sourceFile);
      const interfaceName = member.type.getText(sourceFile);
      if (!tagName || !interfaceName.endsWith("Attributes")) continue;
      if (interfaceName === "SVGAttributes" || interfaceName.endsWith("SVGAttributes")) continue;
      mapping.set(tagName, interfaceName);
    }
  });

  return mapping;
}

function renderMember(member) {
  return `    RawIntrinsicMember { name: ${JSON.stringify(member.name)}, kind: RawIntrinsicMemberKind::${member.kind}, raw_type: ${JSON.stringify(member.rawType)} },`;
}

function generate(root) {
  ts = loadTypeScript(root);
  const dtsPath = resolveRuntimeDomDts(root);
  const runtimeDomPkg = resolveRuntimeDomPackage(root);
  const sourceText = fs.readFileSync(dtsPath, "utf8");
  const sourceFile = ts.createSourceFile(dtsPath, sourceText, ts.ScriptTarget.Latest, true);

  const interfaces = collectInterfaces(sourceFile);
  const resolvedInterfaces = buildResolvedInterfaces(interfaces);
  const tagInterfaces = collectTagInterfaces(sourceFile);

  const renderedInterfaces = Array.from(
    new Set(["HTMLAttributes", ...tagInterfaces.values()]),
  ).sort(stableCompare);

  const lines = [];
  lines.push("/* This file is auto-generated by scripts/generate-html-intrinsics.js */");
  if (runtimeDomPkg?.version) {
    lines.push(`/* Source: @vue/runtime-dom ${runtimeDomPkg.version} */`);
  }
  lines.push("");

  for (const interfaceName of renderedInterfaces) {
    const members = resolvedInterfaces.get(interfaceName) ?? [];
    const constId = `${constName(interfaceName)}_MEMBERS`;
    lines.push(`pub(crate) const ${constId}: &[RawIntrinsicMember] = &[`);
    for (const member of members) {
      lines.push(renderMember(member));
    }
    lines.push("];");
    lines.push("");
  }

  lines.push("/// Every generated member table, in generated (sorted-interface) order — the");
  lines.push("/// deterministic iteration the static intrinsic catalog interns shapes from.");
  lines.push("pub(crate) const ALL_MEMBER_TABLES: &[&[RawIntrinsicMember]] = &[");
  for (const interfaceName of renderedInterfaces) {
    lines.push(`    ${constName(interfaceName)}_MEMBERS,`);
  }
  lines.push("];");
  lines.push("");

  const sortedTags = Array.from(tagInterfaces.entries()).sort(([leftTag], [rightTag]) =>
    stableCompare(leftTag, rightTag),
  );

  lines.push("pub(crate) fn raw_members_for_tag(tag: &str) -> &'static [RawIntrinsicMember] {");
  lines.push("    match tag {");
  for (const [tag, interfaceName] of sortedTags) {
    lines.push(`        ${JSON.stringify(tag)} => ${constName(interfaceName)}_MEMBERS,`);
  }
  lines.push("        _ => HTML_ATTRIBUTES_MEMBERS,");
  lines.push("    }");
  lines.push("}");
  lines.push("");

  const outPath = path.join(
    root,
    "crates",
    "verter_semantic",
    "src",
    "analysis",
    "html_intrinsics_data.rs",
  );
  fs.writeFileSync(outPath, lines.join("\n"));
  return outPath;
}

if (require.main === module) {
  const root = path.resolve(__dirname, "..");
  const out = generate(root);
  console.log(`Generated ${path.relative(root, out)}`);
}

module.exports = { generate };
