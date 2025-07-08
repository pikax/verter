import {
  ParsedBlockScript,
  TemplateBinding,
  TemplateItem,
  TemplateTypes,
} from "../../../../parser";
import { ResolveOptionsFilename } from "../../../script";
import { generateImport } from "../../../utils";
import { TemplatePlugin } from "../../template";
import { Slots } from "../../helpers/component";
import { DEBUG } from "../../../../config";

export const ContextPlugin = {
  name: "VerterContext",
  pre(s, ctx) {
    const isSetup =
      ctx.blocks.find(
        (x) => x.type === "script" && x.block.tag.attributes.setup
      ) !== undefined;

    const options = ResolveOptionsFilename(ctx);

    const TemplateBindingName = ctx.prefix("TemplateBinding");
    const FullContextName = ctx.prefix("FullContext");
    const DefaultName = ctx.prefix("default");
    const ComponentInstanceName = ctx.prefix("ComponentInstance");

    const macros = isSetup
      ? [
          [ctx.prefix("resolveProps"), "$props"],
          [ctx.prefix("resolveEmits"), "$emit"],
          // [ctx.prefix('resolveSlots'), "$slots"],
          [ctx.prefix("defineSlots"), "$slots"],
        ]
      : [];

    const importStr = generateImport([
      {
        from: `./${options}`,
        items: [
          { name: TemplateBindingName },
          { name: FullContextName },
          {
            name: DefaultName,
          },
          ...macros.map(([name]) => ({ name })),
        ],
      },
    ]);

    s.prepend(`${importStr}\n`);

    const instanceStr = `const ${ComponentInstanceName} = new ${DefaultName}();`;
    const CTX = ctx.retrieveAccessor("ctx");
    const generic = ctx.generic ? `<${ctx.generic.names.join(",")}>` : "";

    // todo add generic information
    const ctxItems = [
      isSetup ? ctx.prefix("resolveProps") : null,
      FullContextName,
      TemplateBindingName,
    ]
      .filter(Boolean)
      .map((x) => (ctx.isTS ? `${x}${generic}` : x))
      .map((x) => (ctx.isTS ? `...({} as ${x})` : `...${x}`));
    const ctxStr = `const ${CTX} = {${[
      `...${ComponentInstanceName}`,
      `...${
        ctx.isTS
          ? `({} as Required<typeof ${DefaultName}.components> & {})`
          : `${DefaultName}.components`
      }`,
      // `...${macros.map(([name, prop]) => `${name}(${prop})`).join(",")}`,
      ...macros.map(
        ([name, prop]) =>
          `${prop}: ${ctx.isTS ? `{} as ${name}${generic} & {}` : name}`
      ),
      ...ctxItems,
    ].join(",")}};`;

    const slotsCtx = `const ${ctx.prefix("$slot")} = ${CTX}['$slots'];`;

    const debuggers = DEBUG
      ? [
          `const ___DEBUG_Verter = ${CTX};`,
          "const ___DEBUG_Default = ___VERTER___default;",
          `const ___DEBUG_Props = ({} as ___VERTER___resolveProps${generic});`,
          `const ___DEBUG_Components = ({} as Required<typeof ___VERTER___default.components> & {});`,
          `const ___DEBUG_FullContext = ({} as ___VERTER___FullContext${generic});`,
          `const ___DEBUG_Binding = ({} as ___VERTER___TemplateBinding${generic});`,
        ].join("\n")
      : "";
    s.prependLeft(
      ctx.block.block.block.loc.start.offset,
      [instanceStr, ctxStr, slotsCtx, debuggers].join("\n")
    );

    // add slots Helper
    // s.append(Slots.withPrefix(ctx.prefix("")).content);

    s.append(`
      declare function ${ctx.prefix(
        "slotRender"
      )}<T extends (...args: any[]) => any>(slot: T): (cb: T)=>any;
export declare function ${ctx.prefix("StrictRenderSlot")}<
  T extends (...args: any[]) => any,
  Single extends boolean = ReturnType<T> extends Array<any> ? false : true
>(slot: T, children: Single extends true ? [ReturnType<T>] : ReturnType<T>): any;`);
    // patch TSX
    s.prepend(`
  import { VNode as $V_VNode, ComponentInternalInstance as $V_ComponentInternalInstance } from 'vue';
  import 'vue/jsx';
  declare module "vue" {
    interface HTMLAttributes {
      // "v-slot"?: (instance: HTMLElement) => any;
    }
    interface HTMLAttributes {
      "v-slot"?: (instance: HTMLElement) => any;
    }
    interface InputHTMLAttributes {
      "v-slot"?: (instance: HTMLInputElement) => any;
      onInput?: (e: Event & { target: HTMLInputElement }) => void;
    }
    interface SelectHTMLAttributes {
      onInput?: (e: Event & { target: HTMLSelectElement }) => void;
    }
  }
  
  // TODO improve these types
  // this should contain almost all the component information
  type $V_ToVNode<T> = $V_VNode & {
    ctx: $V_ComponentInternalInstance & { proxy: T };
  };
  declare const $V_instancePropertySymbol: unique symbol;
  
  // patching elements
  declare global {
    namespace JSX {
      export interface IntrinsicClassAttributes<T> {
        "v-slot"?:
          | (T extends { $slots: infer S } ? S : undefined)
          | ((c: T) => T extends { $slots: infer S } ? S : undefined);
  
        "onVue:mounted"?: (vnode: $V_ToVNode<T>) => void;
        "onVue:unmounted"?: (vnode: $V_ToVNode<T>) => void;
        "onVue:updated"?: (vnode: $V_ToVNode<T>, old: $V_ToVNode<T>) => void;
        "onVue:before-mounted"?: (vnode: $V_ToVNode<T>) => void;
        "onVue:before-unmounted"?: (vnode: $V_ToVNode<T>) => void;
        "onVue:before-updated"?: (
          vnode: $V_ToVNode<T>,
          old: $V_ToVNode<T>
        ) => void;
      }
  
      interface LiHTMLAttributes {
        [$V_instancePropertySymbol]?(i: HTMLLIElement): any;
      }
  
      interface MeterHTMLAttributes {
        [$V_instancePropertySymbol]?(i: HTMLMeterElement): any;
      }
      interface OptionHTMLAttributes {
        [$V_instancePropertySymbol]?(i: HTMLOptionElement): any;
      }
      interface ParamHTMLAttributes {
        [$V_instancePropertySymbol]?(i: HTMLParamElement): any;
      }
      interface ProgressHTMLAttributes {
        [$V_instancePropertySymbol]?(i: HTMLProgressElement): any;
      }
      interface SelectHTMLAttributes {
        [$V_instancePropertySymbol]?(i: HTMLSelectElement): any;
      }
      interface TextareaHTMLAttributes {
        [$V_instancePropertySymbol]?(i: TextareaHTMLAttributes): any;
      }
    }
  }
  `);
  },
} as TemplatePlugin;
