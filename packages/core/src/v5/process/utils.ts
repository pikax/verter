import {
  ProcessItemImport,
  ProcessItemType,
  type ImportItem,
  type ImportModule,
} from "./types";
import { ExportedTypes } from "@verter/types/string";
import type { AvailableExports } from "@verter/types/string";

export function defaultPrefix<T>(str: string) {
  return "___VERTER___" + str;
}

export const VERTER_HELPERS_IMPORT = "$verter/types$";

export function createHelperImport(
  items: Array<AvailableExports>,
  prefix: (str: string) => string
): ProcessItemImport {
  return {
    type: ProcessItemType.Import,
    from: VERTER_HELPERS_IMPORT,
    items: items.map((name) => ({
      name: name,
      type: ExportedTypes.has(name),
      alias: prefix(name),
    })),
  };
}

/**
 * @deprecated
 * @param source
 * @returns
 */
function retriveImportFromHelpers(source: string): ImportModule[] {
  const VERTER_IMPORTS_KEY = "__VERTER_IMPORTS__";

  const startStr = `/* ${VERTER_IMPORTS_KEY}`;
  const endStr = `/${VERTER_IMPORTS_KEY} */`;
  const start = source.indexOf(startStr);
  if (start === -1) return [];
  const end = source.indexOf(endStr, start);
  if (end === -1) {
    throw new Error(endStr + ": not found");
  }
  const content = source.slice(start + startStr.length, end);

  const items = JSON.parse(content);
  return items as ImportModule[];
}

/**
 * @deprecated
 * @param source
 * @returns
 */
export function handleHelpers(source: string) {
  const VERTER_START = "__VERTER__START__";

  const imports = retriveImportFromHelpers(source);

  const content = source
    .slice(source.indexOf(VERTER_START) + VERTER_START.length)
    .trim();

  function withPrefix(prefix: string) {
    return {
      content: content.replaceAll("$V_", prefix).replaceAll("\nexport ", "\n"),
      imports: imports.map(
        (i) =>
          ({
            ...i,
            items: i.items?.map((i) => ({
              ...i,
              alias: i.alias ? i.alias.replaceAll("$V_", prefix) : undefined,
            })),
          } as ImportModule)
      ),
    };
  }

  return {
    content,

    imports,

    withPrefix,
  };
}

export function generateImport(items: ImportModule[]) {
  const grouped: Record<string, ImportItem[]> = {};
  for (let i = 0; i < items.length; i++) {
    const item = items[i];

    if (!grouped[item.from]) {
      grouped[item.from] = [];
    }

    const list = grouped[item.from];

    if (!item.items) {
      continue;
    }
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
    if (value.length === 0) {
      imports.push(`import "${key}";`);
      continue;
    }
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
            (i.type ? `type ` : "") + i.name + (i.alias ? ` as ${i.alias}` : "")
        )
        .join(", ")} } from "${key}";`
    );
  }
  return imports.join("\n");
}
