import { VerterASTNode } from "../../../../parser";
import {
  ProcessItem,
  ProcessItemBinding,
  ProcessItemMacroBinding,
  ProcessItemType,
} from "../../../types";
import { handleHelpers } from "../../../utils";
import { definePlugin, ScriptContext } from "../../types";
import { generateTypeString } from "../utils";

export const TemplateBindingPlugin = definePlugin({
  name: "VerterTemplateBinding",
  enforce: "post",

  post(s, ctx) {
    const tag = ctx.block.block.tag;
    const name = ctx.prefix("TemplateBinding");

    // if (!ctx.isSetup) {
    //   const declaration = `function ${name}FN(){return {}}`;

    //   const typeStr = generateTypeString(
    //     name,
    //     {
    //       from: `${name}FN`,
    //       isFunction: true,
    //     },
    //     ctx
    //   );

    //   s.prependRight(tag.pos.close.end, ["", declaration, typeStr].join(";"));
    //   return;
    // }

    const helpers = generateHelpers(ctx.prefix);
    // SETUP MODE

    const templateBindings = new Set(ctx.templateBindings.map((x) => x.name));
    const usedBindings = ctx.items
      .filter((x) => isUsedBinding(x, templateBindings))
      .map((x) => bindingToReturnStatement(x.name, x.name, x.node, ctx));

    const macros = ctx.items
      .filter((x) => x.type === ProcessItemType.MacroBinding)
      .map((x) => macroBindingToReturnStatement(x, ctx));
    const models = ctx.items.filter(
      (x) => x.type === ProcessItemType.DefineModel
    );
    const options = ctx.items.find((x) => x.type === ProcessItemType.Options);

    // known bindings
  },
});

export function bindingToReturnStatement(
  name: string,
  varName: string,
  node: {
    start: number;
    end: number;
  },
  ctx: ScriptContext
) {
  const unref = ctx.prefix(ctx.isTS ? "unref" : "UnwrapRef");

  const value = ctx.isTS
    ? `${name} as unknown as ${unref}<typeof ${name}>`
    : `${unref}(${varName})`;

  return `${name}/*${node.start},${node.end}*/:${value}`;
}

function macroBindingToReturnStatement(
  item: ProcessItemMacroBinding,
  ctx: ScriptContext
) {
  const unref = ctx.prefix(ctx.isTS ? "UnwrapRef" : "unref");
  const ExtractRawType = ctx.prefix("ExtractRawType");

  const value = ctx.isTS
    ? `value/*${item.node.start},${item.node.end}*/:typeof ${item.name}`
    : `value/*${item.node.start},${item.node.end}*/:${item.name}`;

  const items = [value];

  if (item.declarationName) {
    let declaration = "";
    if (item.declarationType === "object") {
      declaration = ctx.isTS
        ? `object:${ExtractRawType}<typeof ${item.declarationName}>`
        : `object:${item.declarationName}`;
    } else if (item.declarationType === "type") {
      declaration = `type:${item.declarationName}`;
    }
    if (declaration) {
      items.push(declaration);
    }
  }

  return `${item.macro}/*${item.node.start},${item.node.end}*/:{${items.join(
    ","
  )}}`;
}

function isUsedBinding(
  item: ProcessItem,
  usedBinding: Set<string | undefined>
): item is ProcessItemBinding {
  return item.type === ProcessItemType.Binding && usedBinding.has(item.name);
}

function generateHelpers(prefix: (s: string) => string) {
  const RawTypeSymbol = prefix("RawTypeSymbol");
  const ToRawType = prefix("ToRawType");
  const ExtractRawType = prefix("ExtractRawType");
  const extractRaw = prefix("extractRaw");

  const defineEmits = prefix("defineEmits");
  const defineProps = prefix("defineProps");
  const defineExpose = prefix("defineExpose");
  const defineSlots = prefix("defineSlots");
  const defineOptions = prefix("defineOptions");

  return `declare const ${RawTypeSymbol}: unique symbol
type ${ToRawType}<T, O = T> = T & { [${RawTypeSymbol}]: O }
type ${ExtractRawType}<T> = T extends { [${RawTypeSymbol}]: infer R } ? R : never
declare function ${extractRaw}<T>(o: T): T extends ${ToRawType}<infer V> ? V : T

declare function ${defineEmits}<EE extends string = string>(
  emitOptions: EE[],
): ${ToRawType}<EE[], EE>
declare function ${defineEmits}<E extends import('vue').EmitsOptions = import('vue').EmitsOptions>(
  emitOptions: E,
): ${ToRawType}<E>
declare function ${defineEmits}<T extends import('vue').ComponentTypeEmits>(): ${ToRawType}<T>
declare function ${defineProps}<PropNames extends string = string>(
  props: PropNames[],
): ${ToRawType}<PropNames[], PropNames>
declare function ${defineProps}<
  PP extends import('vue').ComponentObjectPropsOptions = import('vue').ComponentObjectPropsOptions,
>(props: PP): ${ToRawType}<PP>
declare function ${defineProps}<TypeProps>(): ${ToRawType}<TypeProps>
declare function ${defineExpose}<Exposed extends Record<string, any> = Record<string, any>>(exposed?: Exposed): ${ToRawType}<Exposed>
declare function ${defineOptions}<
  T extends import('vue').ComponentOptionsBase<{}, RawBindings, D, C, M, Mixin, Extends, {}> & {name: Name,props?: never,emits?: never,expose?: never,slots?: never},RawBindings = {},D = {},
  C extends import('vue').ComputedOptions = {},
  M extends import('vue').MethodOptions = {},
  Mixin extends import('vue').ComponentOptionsMixin = import('vue').ComponentOptionsMixin,
  Extends extends import('vue').ComponentOptionsMixin = import('vue').ComponentOptionsMixin,
  Name extends string = string
>(
  options?: T
): ${ToRawType}<T>
declare function ${defineSlots}<
  S extends Record<string, any> = Record<string, any>,
>(): ${ToRawType}<S>`;
}
