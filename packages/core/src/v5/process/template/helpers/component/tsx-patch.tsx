import { VNode as $V_VNode, ComponentInternalInstance as $V_ComponentInternalInstance } from "vue";


/* __VERTER_IMPORTS__
[
    {
        "name": "VNode",
        "source": "vue",
        "default": false
    },
    {
        "name": "ComponentInternalInstance",
        "source": "vue",
        "default": false
    },
]
/__VERTER_IMPORTS__ */

// __VERTER__START__

declare module "vue" {
  interface HTMLAttributes {
    "v-slot"?: {
      default: () => JSX.Element;
    };
  }
  interface InputHTMLAttributes {
    onInput?: (e: Event & {target: HTMLInputElement}) => void;
  }
  interface SelectHTMLAttributes {
    onInput?: (e: Event & {target: HTMLSelectElement}) => void;
  }
}

// TODO improve these types
// this should contain almost all the component information
type $V_ToVNode<T> = $V_VNode & { ctx: $V_ComponentInternalInstance & { proxy: T } };

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
      "onVue:before-updated"?: (vnode: $V_ToVNode<T>, old: $V_ToVNode<T>) => void;
    }
  }
}
