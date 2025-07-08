import { ParsedBlockScript } from "../../../../parser/types";
import { ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";
import { generateTypeDeclaration } from "../utils";

// provides the resolvers for the script block
// eg: resolveProps, resolveEmits, ...
export const ScriptResolversPlugin = definePlugin({
  name: "VerterResolvers",

  // clean the script tag
  post(s, ctx) {
    const tag = ctx.block.block.tag;
    const isTS = ctx.isTS;
    const genericNames = ctx.generic ? `<${ctx.generic.names.join(",")}>` : "";

    const hasModel = ctx.items.some(
      (x) => x.type === ProcessItemType.DefineModel
    );

    const hasEmit = ctx.items.some(
      (x) =>
        x.type === ProcessItemType.MacroBinding && x.macro === "defineEmits"
    );

    const definePropsName = ctx.prefix("defineProps");
    const defineEmitsName = ctx.prefix("defineEmits");
    const defineModelName = ctx.prefix("defineModel");

    const resolvePropsName = ctx.prefix("resolveProps");
    const resolveEmitsName = ctx.prefix("resolveEmits");

    const modelToProp = `{ readonly [K in keyof ${defineModelName}]: ${defineModelName}${genericNames}[K] extends { value: infer V } ? V : never }`;
    const modelToEmits = `{[K in keyof ${defineModelName}]?: ${defineModelName}${genericNames}[K] extends { value: infer V } ? (event: \`update:\${K}\`,value: V) => void : never }[keyof ${defineModelName}]`;

    const emitsToProps = `(${resolveEmitsName}${genericNames} extends (...args: infer Args extends any[]) => void ? {
        [K in Args[0] as \`on\${Capitalize<Args[0]>}\`]?: (...args: Args extends [e: infer E, ...args: infer P]
                ? K extends E
                ? P
                : never
                : never) => any
} : {})`;


    const resolveProps = [
      hasModel
        ? `Omit<${definePropsName}${genericNames}, keyof ${modelToProp}>`
        : `${definePropsName}${genericNames}`,
      hasEmit && emitsToProps,
      hasModel && modelToProp,
    ]
      .filter(Boolean)
      .join(" & ");
    const resolveEmits = [`${defineEmitsName}${genericNames}`, hasModel && modelToEmits]
      .filter(Boolean)
      .join(" & ");

    s.append(
      [
        generateTypeDeclaration(
          resolvePropsName,
          resolveProps,
          ctx.generic?.source,
          ctx.isTS
        ),
        hasEmit &&
          generateTypeDeclaration(
            resolveEmitsName,
            resolveEmits,
            ctx.generic?.source,
            ctx.isTS
          ),
        ,
      ]
        .filter(Boolean)
        .join(";")
    );
  },
});
