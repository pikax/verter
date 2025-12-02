import { MagicString } from "@vue/compiler-sfc";
// import { createContext } from "../parser";
import { parser } from "../v5/parser";
// import { processRender } from "./__old__";
import { basename } from "node:path";

export function parseOld(content: string) {
  // const context = createContext(content);
  // const render = processRender(context);

  return {
    render: {} as any,
  };
}

export function parse(content: string) {
  const parsed = parser(content);
  const s = new MagicString(content);

  // const templateBlock = parsed.blocks.find(
  //   (x) => x.type === "template"
  // ) as ParsedBlockTemplate;
  // const render = processTemplate(
  //   templateBlock.result.items,
  //   [
  //     ...DefaultPlugins,
  //     // clean template tag
  //     //   {
  //     //     post: (s) => {
  //     //       s.update(0, "<template>".length, "");
  //     //       // s.remove(0, "<template>".length);
  //     //       s.update(source.length - "</template>".length, source.length, "");
  //     //     },
  //     //   },
  //   ],
  //   {
  //     //   ...options,
  //     s,
  //     filename: "test.vue",
  //     blocks: parsed.blocks,
  //   }
  // );

  return {
    render: {} as any,
  };
}

export const ValidFiles = Object.entries(
  // @ts-expect-error not the correct flag on the tsconfig
  //   import.meta.glob("./../v5/parser/ast/__bench__/*.ts", {
  import.meta.glob("./__bench__/*.vue", {
    query: "?raw",
    eager: true,
    import: "default",
  }) as Record<string, string>
).reduce((c, [k, v]) => {
  c[basename(k)] = v;
  return c;
}, {} as Record<string, string>);
