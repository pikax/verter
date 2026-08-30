import VueTypeScriptCase from "./vue/TypeScriptCase.vue";
import VueJavaScriptCase from "./vue/JavaScriptCase.vue";
import SvelteTypeScriptCase from "./svelte/TypeScriptCase.svelte";
import SvelteJavaScriptCase from "./svelte/JavaScriptCase.svelte";

export const directVueTs = VueTypeScriptCase;
export const directVueJs = VueJavaScriptCase;
export const directSvelteTs = SvelteTypeScriptCase;
export const directSvelteJs = SvelteJavaScriptCase;

type ExactType<Left, Right> =
  (<Value>() => Value extends Left ? 1 : 2) extends <Value>() => Value extends Right ? 1 : 2
    ? true
    : false;
type AssertType<Condition extends true> = Condition;

export type DirectSvelteTypeScriptFocus = AssertType<
  ExactType<ReturnType<typeof SvelteTypeScriptCase>["focus"], () => void>
>;
export type DirectSvelteJavaScriptFocus = AssertType<
  ExactType<ReturnType<typeof SvelteJavaScriptCase>["focus"], () => void>
>;
