import { ParseScriptContext } from "@verter/core";

export type BlockType =
  | "bundle"
  | "render"
  | "script"
  | "options"
  | "style"
  | "custom";

// const BlockExtensionMap = {
//     render: 'render.tsx',
//     script: 'script',
//     style: 'css',
//     custom: 'custom',
// } satisfies Record<BlockType, string>;

export function getBlockFilename(
  block: BlockType,
  context: ParseScriptContext
) {
  switch (block) {
    case "bundle":
      return context.filename + ".bundle.tsx";
    case "render":
      return context.filename + ".render.tsx";
    case "script":
      return context.filename + ".script.ts";
    case "options":
      return context.filename + ".options." + (context.script?.lang ?? "js");
  }
  return context.filename + ".TODO.tsx";
}

const possibleEndings = [
  "vue.bundle.tsx",
  "vue.render.tsx",
  "vue.script.ts",
  "vue.options.js",
  "vue.options.jsx",
  "vue.options.ts",
  "vue.options.tsx",
];
export function isFileInVueBlock(uri: string) {
  return possibleEndings.some((ending) => uri.endsWith(ending));
}
export function retrieveVueFileFromBlockUri(uri: string) {
  for (const ending of possibleEndings) {
    if (uri.endsWith(ending)) {
      return uri.slice(0, -ending.length);
    }
  }
  return uri;
}
