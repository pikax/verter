import "vue/jsx";
import "./tsx.attributes";

type VNode<T> = import("vue").VNode & {
  ctx: import("vue").ComponentInternalInstance & { proxy: T };
};

declare global {
  namespace JSX {
    export interface IntrinsicClassAttributes<T> {
      "v-slot"?:
        | (T extends { $slots: infer S } ? S : undefined)
        | ((c: T) => T extends { $slots: infer S } ? S : undefined);

      "onVue:mounted"?: (vnode: VNode<T>) => void;
      "onVue:unmounted"?: (vnode: VNode<T>) => void;
      "onVue:updated"?: (vnode: VNode<T>, old: VNode<T>) => void;
      "onVue:before-mounted"?: (vnode: VNode<T>) => void;
      "onVue:before-unmounted"?: (vnode: VNode<T>) => void;
      "onVue:before-updated"?: (vnode: VNode<T>, old: VNode<T>) => void;
    }
  }
}
