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

export type VOnValidModifiersObject<TEvent, TInstance> = {
  stop: StopGuard<TEvent>;
  prevent: PreventGuard<TEvent>;
  self: SelfGuard<TEvent>;

  ctrl: CtrlGuard<TEvent>;
  shift: ShiftGuard<TEvent>;
  alt: AltGuard<TEvent>;
  meta: MetaGuard<TEvent>;

  left: LeftGuard<TEvent>;
  right: RightGuard<TEvent>;
  middle: MiddleGuard<TEvent>;

  exact: ExactGard<TEvent>;

  once: true;
} & ExactKeyModifier<TEvent> &
  DomModifiers<TEvent, TInstance>;

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

export type vOnModifiers<TArg, TInstance = {}> = VOnValidModifiersObject<
  TArg,
  TInstance
> extends infer O
  ? { [K in keyof O as O[K] extends true ? K : never]: O[K] }
  : never;

// /vOn

// vText
export type vTextModifiers<TArg, TInstance = {}> = TArg extends {
  textContent: string | null;
}
  ? {}
  : never;
// /vText

// vHtml
export type vHtmlModifiers<TArg, TInstance = {}> = TArg extends {
  innerHTML: string | null;
}
  ? {}
  : never;
// /vHtml

// vShow
export type vShowModifiers<TArg, TInstance = {}> = TArg extends {
  style: CSSStyleDeclaration;
}
  ? {}
  : never;
// /vShow

// vBind

export type vBindModifiers<TArg, TInstance = {}> = TInstance extends HTMLElement
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

export type vModelModifiers<TArg, TInstance = {}> = TInstance extends
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
