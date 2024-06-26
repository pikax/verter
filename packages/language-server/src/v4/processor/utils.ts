import type { ParseContext, ParseScriptContext } from "@verter/core";

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
      return filename + ".options." + (lang?.replace("js", "ts") ?? "ts");
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
