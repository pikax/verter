/// <reference path="./jsx.d.ts" />
import { defineAsyncComponent } from "vue";
import {
  asyncComponent,
  globalComponentsNav as ___VERTER___globalComponentsNav,
  type GlobalComponentType as ___VERTER___GlobalComponentType,
} from "@verter/types";
import { Local } from "./widgets";
import * as Icons from "./barrel";

type __VerterDynamicProps<C> = C extends abstract new (props: infer P, ...args: any[]) => unknown
  ? P
  : C extends (props: infer P, ...args: any[]) => unknown
    ? P
    : never;
type __VerterDynamicMismatch<C, P> = C extends unknown
  ? P extends __VerterDynamicProps<C>
    ? never
    : C
  : never;
type __VerterDynamicBad<T, CK extends keyof T, PK extends keyof T> = T extends unknown
  ? [T[CK]] extends [never]
    ? T
    : [T[PK]] extends [never]
      ? T
      : [__VerterDynamicMismatch<T[CK], T[PK]>] extends [never]
        ? never
        : T
  : never;
type __VerterDynamicInstance<T, CK extends keyof T> = T extends unknown
  ? T[CK] extends abstract new (...args: any[]) => infer I
    ? I
    : T[CK] extends (...args: any[]) => infer R
      ? R
      : never
  : never;
declare function __VerterDynamicCorrelated<T, CK extends keyof T, PK extends keyof T>(
  choice: T & ([__VerterDynamicBad<T, CK, PK>] extends [never] ? unknown : never),
  componentKey: CK,
  propsKey: PK,
): __VerterDynamicInstance<T, CK>;

type WrongChoice =
  | { component: typeof Local; props: { kind: "card"; label: string } }
  | { component: typeof Icons.Button; props: { kind: "card"; label: string } };
declare const wrong: WrongChoice;
__VerterDynamicCorrelated(wrong, "component", "props");

type HiddenUnion = {
  component: typeof Local | typeof Icons.Button;
  props: { kind: "card"; label: string };
};
declare const hiddenUnion: HiddenUnion;
__VerterDynamicCorrelated(hiddenUnion, "component", "props");

declare const impossible: { component: never; props: { kind: "card"; label: string } };
__VerterDynamicCorrelated(impossible, "component", "props");

const MissingThing = {} as ___VERTER___GlobalComponentType<"MissingThing">;
void ___VERTER___globalComponentsNav().MissingThing;
new MissingThing({});

const AsyncCard = asyncComponent(defineAsyncComponent(() => Promise.resolve(Local)));
const badAsyncProps: InstanceType<typeof AsyncCard>["$props"] = { kind: "card", label: true };
void badAsyncProps;
declare const Generic: new <T>(props: { value: T }) => {
  readonly $props: { value: T };
  readonly value: T;
};
const AsyncGeneric = asyncComponent(defineAsyncComponent(() => Promise.resolve(Generic)));
const wrongGenericValue: string = new AsyncGeneric({ value: 42 }).value;
void wrongGenericValue;
