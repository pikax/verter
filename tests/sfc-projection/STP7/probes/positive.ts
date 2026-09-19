/**
 * Svelte 5 public Component shape: a function, not a Vue constructor.
 * Shared origin/binder fixtures: generic component, each-scope, snippet.
 */

import type { Component } from "svelte";

/**
 * The fixture's component shape derives from the installed Svelte
 * `Component` type, so `Widget` is type-checked against the public contract
 * the product evidence names rather than a local replica of it.
 */
export interface Svelte5Component<
  Props extends Record<string, unknown> = Record<string, never>,
  Exports extends Record<string, unknown> = Record<string, never>,
> extends Component<Props, Exports> {}

export type Item = { id: number; label: string };

export const Widget: Svelte5Component<{ item: Item }, { peek: () => Item }> = (
  _internals,
  props,
) => ({
  peek: () => props.item,
});

export function eachScope<T>(items: T[], visit: (item: T, index: number) => T): T[] {
  return items.map(visit);
}

export type Snippet<Args extends unknown[] = []> = (
  this: void,
  ...args: Args
) => { readonly "{@render}": true };

export const greetSnippet: Snippet<[who: string]> = (_who) => ({
  "{@render}": true as const,
});

const items: Item[] = [{ id: 1, label: "a" }];
export const eachResult = eachScope(items, (item) => item);

/** STP7-hover */
export const stp7HoverTarget: number = eachResult[0].id;

export const stp7DefinitionTarget: typeof Widget = Widget;

void greetSnippet;
