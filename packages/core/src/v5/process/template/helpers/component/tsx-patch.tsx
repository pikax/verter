import "vue/jsx";
import {
  renderList as ___VERTER___renderList,
  normalizeClass as ___VERTER___normalizeClass,
  normalizeStyle as ___VERTER___normalizeStyle,
  defineComponent as ___VERTER___defineComponent,
  type ShallowUnwrapRef as ___VERTER___ShallowUnwrapRef,
  type GlobalComponents as ___VERTER___GlobalComponents,
  type ComponentPublicInstance as ___VERTER___ComponentPublicInstance,
  type Ref as ___VERTER___Ref,
  type IntrinsicElementAttributes as ___VERTER___IntrinsicElementAttributes,
  type Component as ___VERTER___VueComponent,
} from "vue";

import type {
  Suspense as ___VERTER___Suspense,
  KeepAlive as ___VERTER___KeepAlive,
  Transition as ___VERTER___Transition,
  TransitionGroup as ___VERTER___TransitionGroup,
  Teleport as ___VERTER___Teleport,
} from "vue";

declare module "vue" {
  interface GlobalComponents {
    Suspense: typeof ___VERTER___Suspense;
    KeepAlive: typeof ___VERTER___KeepAlive;
    Transition: typeof ___VERTER___Transition;
    TransitionGroup: typeof ___VERTER___TransitionGroup;
    Teleport: typeof ___VERTER___Teleport;
  }

  interface HTMLAttributes {
    "v-slot"?: {
      default: () => JSX.Element;
    };
  }
}

// TODO improve these types
// this should contain almost all the component information
type ToVNode<T> = VNode & { ctx: ComponentInternalInstance & { proxy: T } };

// patching elements
declare global {
  namespace JSX {
    export interface IntrinsicClassAttributes<T> {
      "v-slot"?:
        | (T extends { $slots: infer S } ? S : undefined)
        | ((c: T) => T extends { $slots: infer S } ? S : undefined);

      "onVue:mounted"?: (vnode: ToVNode<T>) => void;
      "onVue:unmounted"?: (vnode: ToVNode<T>) => void;
      "onVue:updated"?: (vnode: ToVNode<T>, old: ToVNode<T>) => void;
      "onVue:before-mounted"?: (vnode: ToVNode<T>) => void;
      "onVue:before-unmounted"?: (vnode: ToVNode<T>) => void;
      "onVue:before-updated"?: (vnode: ToVNode<T>, old: ToVNode<T>) => void;
    }
  }
}
