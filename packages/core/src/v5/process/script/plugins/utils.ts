import { GenericInfo } from "../../../../parser";
import { ProcessContext } from "../../types";
import { ScriptContext } from "../types";

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
  }${info.from}${generic ? `<${generic.source}>` : ""}${info.isFunction ? ">" : ""}${
    isAsync || info.key
      ? ` ${isAsync ? "extends Promise<infer R" : ""}${
          info.key ? ` extends { ${info.key}: infer K }` : ""
        }${isAsync ? ">" : ""}?${info.key ? "K" : "R"}:never`
      : ""
  }`;

  if (isTS) {
    return `;export type ${name}${generic ? `<${generic.source}>` : ""}=${content};\n`;
  } else {
    return `\n/** @typedef {${content}} ${name} */\n/** @type {${name}} */\nexport const ${name} = null;\n`;
  }
}
