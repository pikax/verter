import { GenericInfo } from "../../../../parser";
import { ProcessContext } from "../../types";
import { ScriptContext } from "../types";

export function generateTypeDeclaration(
  name: string,
  content: string,
  generic: string | undefined,
  typescript: boolean
) {
  return typescript
    ? `;export type ${name}${generic ? `<${generic}>` : ""}=${content};`
    : `\n/** @typedef {${content}} ${name} */\n/** @type {${name}} */\nexport const ${name} = null;\n`;
}

export function generateTypeString(
  name: string,
  info: {
    from: string;
    key?: string;
    isFunction?: boolean;
    isType?: boolean;
  },
  ctx: ScriptContext
) {
  const isTS = ctx.block.lang.startsWith("ts");
  const isAsync = ctx.isAsync;
  const generic = ctx.generic;

  const content = `${info.isFunction ? "ReturnType<" : ""}${
    info.isType ? "" : "typeof "
  }${info.from}${generic ? `<${generic.source}>` : ""}${
    info.isFunction ? ">" : ""
  }${
    (isAsync && info.isFunction) || info.key
      ? ` ${isAsync && info.isFunction ? "extends Promise<infer R" : ""}${
          info.key ? ` extends { ${info.key}: infer K }` : ""
        }${isAsync && info.isFunction ? ">" : ""}?${info.key ? "K" : "R"}:never`
      : ""
  }`;

  return generateTypeDeclaration(name, content, generic?.source, isTS);
}
