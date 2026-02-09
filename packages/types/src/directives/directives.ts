import { PickByValue } from "../helpers";
import { ExtractFromHTMLElement } from "../tsx/components-tsx";
import { SystemModifiers } from "../vue";

export type ExtractLeafElement<T> = T extends HTMLElement
  ? T
  : T extends { $el: infer E }
    ? ExtractLeafElement<E>
    : T extends { new (): infer I }
      ? ExtractLeafElement<I>
      : never;

// vOn
type StopGuard<T> = T extends { stopPropagation(): void } ? true : false;
type PreventGuard<T> = T extends { preventDefault(): void } ? true : false;
type SelfGuard<T> = T extends { target: any; currentTarget: any } ? true : false;

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

type VOnValidModifiersObject<TInstance, TArg> = {
  stop?: StopGuard<TArg>;
  prevent?: PreventGuard<TArg>;
  self?: SelfGuard<TArg>;

  ctrl?: CtrlGuard<TArg>;
  shift?: ShiftGuard<TArg>;
  alt?: AltGuard<TArg>;
  meta?: MetaGuard<TArg>;

  left?: LeftGuard<TArg>;
  right?: RightGuard<TArg>;
  middle?: MiddleGuard<TArg>;

  exact?: ExactGard<TArg>;

  once?: true;
} & ExactKeyModifier<TArg> &
  DomModifiers<TArg, TInstance>;
type ExactKeyModifier<TEvent> =
  ExactGard<TEvent> extends true
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
  ? ExtractFromHTMLElement<TInstance> extends infer Event
    ? {
        [K in keyof Event]: K extends `on${string}` ? K : never;
      }[keyof Event]
    : never
  : never;
type OnlyEventKeysFromProps<TInstance> = TInstance extends { $props: any }
  ? {
      [K in keyof TInstance["$props"]]: K extends `on${Capitalize<string>}` ? K : never;
    }[keyof TInstance["$props"]]
  : never;

export type vOnModifiers<
  TInstance,
  TName extends TInstance extends { $props: any }
    ? OnlyEventKeysFromProps<TInstance>
    : OnlyEventKeys<TInstance>,
> = TInstance extends HTMLElement
  ? ExtractFromHTMLElement<TInstance> extends infer Event
    ? Event[TName] extends ((e: infer E) => any) | null | undefined
      ? Partial<PickByValue<VOnValidModifiersObject<Event, E>, true | undefined>>
      : {}
    : {}
  : TInstance extends {
        $props: {
          [K in TName]?: ((e: infer E) => any) | undefined;
        };
      }
    ? Partial<PickByValue<VOnValidModifiersObject<TInstance, E>, true | undefined>>
    : {};

// /vOn

// vText
export type vTextModifiers<TInstance> = TInstance extends {
  textContent: string | null;
}
  ? never
  : never;
// /vText

// vHtml
export type vHtmlModifiers<TInstance> = TInstance extends {
  innerHTML: string | null;
}
  ? never
  : never;
// /vHtml

// vShow
export type vShowModifiers<TInstance> = TInstance extends {
  style: CSSStyleDeclaration;
}
  ? never
  : TInstance extends {
        $props: {
          style?: any;
        };
      }
    ? never
    : never;
// /vShow

// vBind

export type vBindModifiers<TInstance, TName> = TInstance extends HTMLElement
  ? {
      prop?: true;
      attr?: true;

      camel?: true;
    }
  : {
      camel?: true;
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
      lazy?: true;
      // TODO this should typecheck for number
      number?: true;
      // TODO this should typecheck for trim-able strings
      trim?: true;
    }
  : {};

// /vModel

// vPre
export type vPreModifiers<TArg, TInstance = {}> = never;
// /vPre
// vOnce
export type vOnceModifiers<TArg, TInstance = {}> = never;
// /vOnce
// vMemo
export type vMemoModifiers<TArg, TInstance = {}> = never;
// /vMemo
// vCloak
export type vCloakModifiers<TArg, TInstance = {}> = never;
// /vCloak

// custom directive

export declare function runCustomDirective<
  TInstance,
  TDirective extends import("vue").Directive<ExtractLeafElement<TInstance>>,
>(
  instance: TInstance,
  directive: TDirective,
): ExtractLeafElement<TInstance> extends infer El extends HTMLElement
  ? TDirective extends import("vue").Directive<infer TElement, infer TValue, infer M extends string>
    ? El extends TElement
      ? (
          instance: TInstance,
          value: TValue,
          arg: string | undefined,
          modifiers: { [K in M]?: true },
        ) => void
      : (
          instance: TElement,
          value: TValue,
          arg: string | undefined,
          modifiers: { [K in M]?: true },
        ) => void
    : false
  : false;
// /custom directive

export type ExtractDirectives<T> = {
  [K in keyof T as T[K] extends import("vue").Directive<any, any, any, any>
    ? K extends `v${Capitalize<string>}`
      ? K
      : never
    : never]: T[K];
};

export declare function retrieveSetupDirectives<T>(
  o: T,
): ExtractDirectives<T> extends infer D
  ? ExtractDirectives<Omit<import("vue").GlobalDirectives, keyof D>> & D
  : ExtractDirectives<import("vue").GlobalDirectives>;

// NOTE for options, it needs to rely on the boxed value types from Vue,
// because the defineComponent is too laxed
