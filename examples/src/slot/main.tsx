import "vue/jsx";
import type { SlotsType } from "vue";
import {
  type VNode as $V_VNode,
  type ComponentInternalInstance as $V_ComponentInternalInstance,
  defineComponent,
} from "vue";
declare module "vue" {
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

// patching elements
// declare global {
//   // namespace JSX {
//   //   export interface IntrinsicClassAttributes<T> {
//   //     // $children: T extends { $slots: infer S } ? { default?: never } & { [K in keyof S]: S[K] extends (...args: infer Args) => any ? (...args: Args) => JSX.Element : () => JSX.Element } : { default?: never }
//   //     // $children?: T extends { $slots: infer S } ? S : undefined
//   //     "v-slot"?:
//   //       | (T extends { $slots: infer S } ? S : undefined)
//   //       | ((c: T) => T extends { $slots: infer S } ? S : undefined);
//   //   }
//   // }
// }
// declare global {
//   namespace JSX {
//     // export interface IntrinsicClassAttributes<T> {
//     //   "v-slot"?:
//     //     | (T extends { $slots: infer S } ? S : undefined)
//     //     | ((c: T) => T extends { $slots: infer S } ? S : undefined);

//     //   "onVue:mounted"?: (vnode: $V_ToVNode<T>) => void;
//     //   "onVue:unmounted"?: (vnode: $V_ToVNode<T>) => void;
//     //   "onVue:updated"?: (vnode: $V_ToVNode<T>, old: $V_ToVNode<T>) => void;
//     //   "onVue:before-mounted"?: (vnode: $V_ToVNode<T>) => void;
//     //   "onVue:before-unmounted"?: (vnode: $V_ToVNode<T>) => void;
//     //   "onVue:before-updated"?: (
//     //     vnode: $V_ToVNode<T>,
//     //     old: $V_ToVNode<T>
//     //   ) => void;
//     // }

//     interface LiHTMLAttributes {
//       [$V_instancePropertySymbol]?(i: HTMLLIElement): any;
//     }

//     interface MeterHTMLAttributes {
//       [$V_instancePropertySymbol]?(i: HTMLMeterElement): any;
//     }
//     interface OptionHTMLAttributes {
//       [$V_instancePropertySymbol]?(i: HTMLOptionElement): any;
//     }
//     interface ParamHTMLAttributes {
//       [$V_instancePropertySymbol]?(i: HTMLParamElement): any;
//     }
//     interface ProgressHTMLAttributes {
//       [$V_instancePropertySymbol]?(i: HTMLProgressElement): any;
//     }
//     interface SelectHTMLAttributes {
//       [$V_instancePropertySymbol]?(i: HTMLSelectElement): any;
//     }
//     interface TextareaHTMLAttributes {
//       [$V_instancePropertySymbol]?(i: TextareaHTMLAttributes): any;
//     }
//   }
// }

// TODO improve these types
// this should contain almost all the component information
type $V_ToVNode<T> = $V_VNode & {
  ctx: $V_ComponentInternalInstance & { proxy: T };
};
declare const $V_instancePropertySymbol: unique symbol;

const Comp = defineComponent({
  props: {
    bar: String,
  },
  slots: {} as SlotsType<{ header: (a: { foo: string }) => void }>,
});

declare const Foo: { new (): { $props: { bar: string } } };

<Comp
  v-slot={(x) => {
    x.bar;
  }}
/>;

<Foo bar="" v-slot={(x) => {}} />;

<select v-slot={} />;
