/// <reference types="vue/jsx" />
import "./tsx.attributes";

type VNode<T> = import("vue").VNode & {
  ctx: import("vue").ComponentInternalInstance & { proxy: T };
};

// Module augmentation for jsxImportSource: "vue"
declare module "vue/jsx-runtime" {
  namespace JSX {
    export interface IntrinsicClassAttributes<T> {
      // // Vue TSX style, in verter only the argument is needed.
      // // leaving this here just in case we want to compile to TSX for other usages
      // "v-slot"?:
      //   | (T extends { $slots: infer S } ? S : undefined)
      //   | ((c: T) => T extends { $slots: infer S } ? S : undefined);

      /**
       * Helper to retrieve the current instance in TSX/JSX
       * ONLY TO BE USED in Verter
       * @param c Type of the component instance
       */
      "v-slot"?: (c: T) => any;

      "onVue:before-create"?: (vnode: VNode<T>) => void;
      "onVue:created"?: (vnode: VNode<T>) => void;
      "onVue:before-mount"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:mounted"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:before-update"?: (vnode: VNode<T>, old: VNode<T>) => void;
      "onVue:updated"?: (vnode: VNode<T>, old: VNode<T>) => void;
      "onVue:before-unmount"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:unmounted"?: (vnode: VNode<T>) => void;
      "onVue:error-captured"?: (vnode: VNode<T>) => void;
      "onVue:render-tracked"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:render-triggered"?: (
        vnode: VNode<T>,
        old: VNode<T> | null
      ) => void;
      "onVue:activated"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:deactivated"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:server-prefetch"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
    }
  }
}

// Global JSX namespace augmentation for jsx: "preserve" without jsxImportSource
declare global {
  namespace JSX {
    export interface IntrinsicClassAttributes<T> {
      /**
       * Helper to retrieve the current instance in TSX/JSX
       * ONLY TO BE USED in Verter
       * @param c Type of the component instance
       */
      "v-slot"?: (c: T) => any;

      "onVue:before-create"?: (vnode: VNode<T>) => void;
      "onVue:created"?: (vnode: VNode<T>) => void;
      "onVue:before-mount"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:mounted"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:before-update"?: (vnode: VNode<T>, old: VNode<T>) => void;
      "onVue:updated"?: (vnode: VNode<T>, old: VNode<T>) => void;
      "onVue:before-unmount"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:unmounted"?: (vnode: VNode<T>) => void;
      "onVue:error-captured"?: (vnode: VNode<T>) => void;
      "onVue:render-tracked"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:render-triggered"?: (
        vnode: VNode<T>,
        old: VNode<T> | null
      ) => void;
      "onVue:activated"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:deactivated"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
      "onVue:server-prefetch"?: (vnode: VNode<T>, old: VNode<T> | null) => void;
    }
  }
}
