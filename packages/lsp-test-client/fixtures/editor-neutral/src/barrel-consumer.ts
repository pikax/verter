import {
  SvelteJavaScriptCase,
  SvelteTypeScriptCase,
  VueJavaScriptCase,
  VueTypeScriptCase,
} from "./public";

export const barrelVueTs = VueTypeScriptCase;
export const barrelVueJs = VueJavaScriptCase;
export const barrelSvelteTs = SvelteTypeScriptCase;
export const barrelSvelteJs = SvelteJavaScriptCase;

type ExactType<Left, Right> =
  (<Value>() => Value extends Left ? 1 : 2) extends <Value>() => Value extends Right ? 1 : 2
    ? true
    : false;
type AssertType<Condition extends true> = Condition;

export type BarrelSvelteTypeScriptFocus = AssertType<
  ExactType<ReturnType<typeof SvelteTypeScriptCase>["focus"], () => void>
>;
export type BarrelSvelteJavaScriptFocus = AssertType<
  ExactType<ReturnType<typeof SvelteJavaScriptCase>["focus"], () => void>
>;
