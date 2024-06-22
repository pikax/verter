import type { ParseScriptContext } from "@verter/core";

export type BlockType =
  | "bundle"
  | "render"
  | "script"
  | "options"
  | "style"
  | "custom";

export function getBlockFilename(
  block: BlockType,
  context: ParseScriptContext,
  asRelative = false
) {
  const filename = asRelative
    ? `./${context.filename.split("/").pop()}`
    : context.filename;

  return blockToFilename(block, filename, context.script?.lang);
}

export function blockToFilename(
  block: BlockType,
  filename: string,
  lang?: string
) {
  switch (block) {
    case "bundle":
      return filename + ".bundle.tsx";
    case "render":
      return filename + ".render.tsx";
    case "script":
      return filename + ".script.ts";
    case "options":
      return filename + ".options." + (lang ?? "js");
  }
  return filename + ".TODO.tsx";
}

const possibleEndings = [
  ".vue.bundle.tsx",
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
export function retrieveVueFileFromBlockUri(uri: string) {
  for (const ending of possibleEndings) {
    if (uri.endsWith(ending)) {
      return uri.slice(0, -ending.length + 4);
    }
  }
  return uri;
}
