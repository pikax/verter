import { PickByValue } from "../helpers";
import { SystemModifiers } from "../vue";

// vOn
type StopGuard<T> = T extends { stopPropagation(): void } ? true : false;
type PreventGuard<T> = T extends { preventDefault(): void } ? true : false;
type SelfGuard<T> = T extends { target: any; currentTarget: any }
  ? true
  : false;

type CtrlGuard<T> = T extends { ctrlKey: boolean } ? true : false;
type ShiftGuard<T> = T extends { shiftKey: boolean } ? true : false;
type AltGuard<T> = T extends { altKey: boolean } ? true : false;
type MetaGuard<T> = T extends { metaKey: boolean } ? true : false;

type LeftGuard<T> = T extends { button: number } ? true : false;
type RightGuard<T> = T extends { button: number } ? true : false;
type MiddleGuard<T> = T extends { button: number } ? true : false;

type ExactGard<T> = T extends {
  [K in `${SystemModifiers}Key`]: boolean;
} & {
  key: string;
}
  ? true
  : false;

export type VOnValidModifiersObject<TInstance, TArg> = {
  stop: StopGuard<TArg>;
  prevent: PreventGuard<TArg>;
  self: SelfGuard<TArg>;

  ctrl: CtrlGuard<TArg>;
  shift: ShiftGuard<TArg>;
  alt: AltGuard<TArg>;
  meta: MetaGuard<TArg>;

  left: LeftGuard<TArg>;
  right: RightGuard<TArg>;
  middle: MiddleGuard<TArg>;

  exact: ExactGard<TArg>;

  once: true;
} & ExactKeyModifier<TArg> &
  DomModifiers<TArg, TInstance>;
type ExactKeyModifier<TEvent> = ExactGard<TEvent> extends true
  ? {
      [K: string]: true;
    }
  : {};

type DomModifiers<TEvent, TInstance> = TEvent extends Event
  ? TInstance extends HTMLElement
    ? {
        passive: true;
        capture: true;
      }
    : {}
  : {};

type OnlyEventKeys<TInstance> = TInstance extends HTMLElement
  ? {
      [K in keyof TInstance]: K extends `on${string}` ? K : never;
    }[keyof TInstance]
  : never;
type OnlyEventKeysFromProps<TInstance> = TInstance extends { $props: any }
  ? {
      [K in keyof TInstance["$props"]]: K extends `on${Capitalize<string>}`
        ? K
        : never;
    }[keyof TInstance["$props"]]
  : never;

export type vOnModifiers<
  TInstance,
  TName extends TInstance extends { $props: any }
    ? OnlyEventKeysFromProps<TInstance>
    : OnlyEventKeys<TInstance>
> = TInstance extends HTMLElement
  ? TInstance[TName] extends ((e: infer E) => any) | null
    ? PickByValue<VOnValidModifiersObject<TInstance, E>, true>
    : {}
  : TInstance extends {
      $props: {
        [K in TName]?: ((e: infer E) => any) | undefined;
      };
    }
  ? PickByValue<VOnValidModifiersObject<TInstance, E>, true>
  : {};

// /vOn

// vText
export type vTextModifiers<TInstance> = TInstance extends {
  textContent: string | null;
}
  ? {}
  : never;
// /vText

// vHtml
export type vHtmlModifiers<TInstance> = TInstance extends {
  innerHTML: string | null;
}
  ? {}
  : never;
// /vHtml

// vShow
export type vShowModifiers<TInstance> = TInstance extends {
  style: CSSStyleDeclaration;
}
  ? {}
  : TInstance extends {
      $props: {
        style?: any;
      };
    }
  ? {}
  : never;
// /vShow

// vBind

export type vBindModifiers<TInstance, TName> = TInstance extends HTMLElement
  ? {
      prop: true;
      attr: true;

      camel: true;
    }
  : {
      camel: true;
    };

// /vBind

// vModel

export type vModelModifiers<TInstance, TName> = TInstance extends
  | HTMLInputElement
  | HTMLSelectElement
  | HTMLTextAreaElement
  | { new (): { $props: any } }
  | (() => any)
  ? {
      // TODO change check if there's an `input` event or maybe this should only be available for HTMLElements
      lazy: true;
      // TODO this should typecheck for number
      number: true;
      // TODO this should typecheck for trim-able strings
      trim: true;
    }
  : {};

// /vModel

// vPre
export type vPreModifiers<TArg, TInstance = {}> = {};
// /vPre
// vOnce
export type vOnceModifiers<TArg, TInstance = {}> = {};
// /vOnce
// vMemo
export type vMemoModifiers<TArg, TInstance = {}> = {};
// /vMemo
// vCloak
export type vCloakModifiers<TArg, TInstance = {}> = {};
// /vCloak

export type CustomDirectiveModifiers<T> = T extends import("vue").Directive<
  any,
  any,
  infer M extends string
>
  ? { [K in M]: true }
  : never;

declare const myDirective: import("vue").Directive<
  HTMLInputElement,
  { color: "black" | "white" },
  "foo" | "bar"
>;

export type MyDirectiveModifiers = CustomDirectiveModifiers<typeof myDirective>;
// type MyDirectiveModifiers = { foo: true; bar: true }
// /CustomDirectiveModifiers

export type ExtractLeafElement<T> = T extends HTMLElement
  ? T
  : T extends { $el: infer E }
  ? ExtractLeafElement<E>
  : T extends { new (): infer I }
  ? ExtractLeafElement<I>
  : never;

declare const bar: {
  $el: {
    $el: HTMLDivElement;
  };
};

declare const baz: {
  new (): {
    $el: {
      $el: {
        $el: HTMLSpanElement;
      };
    };
  };
};

declare const qux: {
  new (): {
    $el: {
      new (): {
        $el: [HTMLButtonElement, HTMLInputElement];
      };
    };
  };
};

declare const inputEl: {
  $el: HTMLInputElement;
};

declare const foo: {
  $el: typeof bar | typeof baz | typeof qux;
};

export type FooElement = ExtractLeafElement<typeof foo>;

export declare function runCustomDirective<
  TInstance,
  TDirective extends import("vue").Directive<ExtractLeafElement<TInstance>>
>(
  instance: TInstance,
  directive: TDirective
): ExtractLeafElement<TInstance> extends infer El extends HTMLElement
  ? TDirective extends import("vue").Directive<
      infer TElement,
      infer TArg,
      infer M extends string
    >
    ? El extends TElement
      ? (instance: TInstance, arg: TArg, modifiers: { [K in M]?: true }) => void
      : (instance: TElement, arg: TArg, modifiers: { [K in M]?: true }) => void
    : false
  : false;

const r = runCustomDirective(foo, myDirective)(
  foo,
  { color: "black" },
  { foo: true, bar: true }
);

runCustomDirective(foo, myDirective)(
  foo,
  { color: "black" },
  { foo: true, bar: true }
);

runCustomDirective(inputEl, myDirective)(
  inputEl,
  { color: "black" },
  { foo: true, bar: true }
);

// export function runDirective<TInstance, TDirective>():  TDirective extends import("vue").Directive<
