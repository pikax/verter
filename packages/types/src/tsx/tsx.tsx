import { PublicInstanceFromMacro } from "../instance/instance";
import { createMacroReturn } from "../setup";
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
