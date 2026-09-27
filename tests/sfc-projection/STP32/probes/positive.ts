/// <reference path="./jsx.d.ts" />
import { defineAsyncComponent } from "vue";
import {
  asyncComponent,
  globalComponentsNav as ___VERTER___globalComponentsNav,
  type GlobalComponentType as ___VERTER___GlobalComponentType,
} from "@verter/types";
import { Local, type CardInstance } from "./widgets";
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

declare module "vue" {
  interface GlobalComponents {
    RegisteredWidget: typeof Local;
  }
}

type Choice =
  | { component: typeof Local; props: { kind: "card"; label: string } }
  | { component: typeof Icons.Button; props: { kind: "icon"; size: number } };
declare const choice: Choice;
const dynamicInstance = __VerterDynamicCorrelated(choice, "component", "props");
declare const authoredOpen: { component: any; props: any };
__VerterDynamicCorrelated(authoredOpen, "component", "props");
type AuthoredOpenBad = __VerterDynamicBad<typeof authoredOpen, "component", "props">;
const authoredAnyRemainsOpen: [AuthoredOpenBad] extends [never] ? true : false = true;
const dynamicLabel: string | undefined =
  "label" in dynamicInstance ? dynamicInstance.label : undefined;

const RegisteredWidget = {} as ___VERTER___GlobalComponentType<"RegisteredWidget">;
void ___VERTER___globalComponentsNav().RegisteredWidget;
const globalInstance = new RegisteredWidget({ kind: "card", label: "global" });
const globalLabel: string = globalInstance.label;

const namespaceInstance = new Icons.Button({ kind: "icon", size: 12 });
const iconSize: number = namespaceInstance.size;

declare const __VerterPublicComponent: typeof Local;
const recursiveInstance = new __VerterPublicComponent({ kind: "card", label: "self" });
const recursiveLabel: string = recursiveInstance.label;

const AsyncCard = asyncComponent(defineAsyncComponent(() => Promise.resolve(Local)));
const asyncProps: InstanceType<typeof AsyncCard>["$props"] = { kind: "card", label: "async" };
declare const Generic: new <T>(props: { value: T }) => {
  readonly $props: { value: T };
  readonly value: T;
};
const AsyncGeneric = asyncComponent(defineAsyncComponent(() => Promise.resolve(Generic)));
const genericInstance = new AsyncGeneric({ value: "generic" });
const genericValue: string = genericInstance.value;

export type Instance = InstanceType<typeof Local>;
export const stp32DefinitionTarget = Local;
export const stp32HoverTarget: number = iconSize;
void stp32DefinitionTarget;
void asyncProps;
void genericValue;
void dynamicLabel;
void authoredAnyRemainsOpen;
void globalLabel;
void recursiveLabel;
export type _Card = CardInstance;
