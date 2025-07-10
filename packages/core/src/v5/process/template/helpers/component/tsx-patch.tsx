import type {
  VNode as $V_VNode,
  ComponentInternalInstance as $V_ComponentInternalInstance,
} from "vue";

/* __VERTER_IMPORTS__
[
  {
    "from": "vue",
    "asType": true,
    "items": [
      { name: "VNode", alias: "$V_VNode" },
      { name: "ComponentInternalInstance", alias: "$V_ComponentInternalInstance" },
    ]
  }
]
/__VERTER_IMPORTS__ */

// __VERTER__START__

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
