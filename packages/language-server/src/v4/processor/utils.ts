import type {
  ParseContext,
  ParseScriptContext,
  TypeLocationImport,
} from "@verter/core";
import { ImportItem } from "@verter/core/dist/plugins";

export type BlockType =
  | "bundle"
  | "render"
  | "script"
  | "options"
  | "style"
  | "custom";

export function getBlockFilename(
  block: BlockType,
  context: ParseContext,
  asRelative = false
) {
  const filename = asRelative
    ? `./${context.filename.split("/").pop()}`
    : context.filename;

  return blockToFilename(
    block,
    filename,
    (context.sfc.scriptSetup ?? context.sfc.script)?.lang
  );
}

export function blockToFilename(
  block: BlockType,
  filename: string,
  lang?: string
) {
  switch (block) {
    case "bundle":
      return filename + ".bundle.ts";
    case "render":
      return filename + ".render.tsx";
    case "script":
      return filename + ".script.ts";
    case "options":
      return filename + ".options." + (lang?.replace("js", "ts") ?? "js");
  }
  return filename + ".TODO.tsx";
}

const possibleEndings = [
  // bundle is not a block of vue, but the representation of the whole vue file
  // ".vue.bundle.ts",
  ".vue.render.tsx",
  ".vue.script.ts",
  ".vue.options.js",
  ".vue.options.jsx",
  ".vue.options.ts",
  ".vue.options.tsx",
];
export function isFileInVueBlock(uri: string) {
  return possibleEndings.some((ending) => uri.endsWith(ending));
}

export function isVueSubDocument(uri: string) {
  return isFileInVueBlock(uri) || uri.endsWith(".vue.bundle.ts");
}

export function retrieveVueFileFromBlockUri(uri: string) {
  if (uri.endsWith(".vue.bundle.ts")) {
    return uri.slice(0, -".bundle.ts".length);
  }
  for (const ending of possibleEndings) {
    if (uri.endsWith(ending)) {
      return uri.slice(0, -ending.length + 4);
    }
  }
  return uri;
}

export function importsLocationsToString(items: TypeLocationImport[]) {
  const grouped: Record<string, ImportItem[]> = {};
  for (let i = 0; i < items.length; i++) {
    const item = items[i];

    if (!grouped[item.from]) {
      grouped[item.from] = [];
    }

    const list = grouped[item.from];

    if (item.asType) {
      // convert pure types to imports
      list.push(...item.items.map((i) => ({ ...i, type: true })));
    } else {
      list.push(...item.items);
    }
  }

  const imports: string[] = [];
  for (const [key, value] of Object.entries(grouped)) {
    const added = new Set<string>();
    const toAdd: ImportItem[] = [];
    for (const item of value) {
      const name = item.alias ?? item.name;
      // ignore duplicates
      if (added.has(name)) {
        continue;
      }
      toAdd.push(item);
      added.add(name);
    }
    imports.push(
      `import { ${toAdd
        .map(
          (i) =>
            (i.type ? `type ${i.name}` : "") +
            i.name +
            (i.alias ? ` as ${i.alias}` : "")
        )
        .join(", ")} } from "${key}";`
    );
  }
  return imports.join("\n");
}
