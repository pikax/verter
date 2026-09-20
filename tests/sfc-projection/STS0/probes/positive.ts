/**
 * STS0 clean twin: the ratified Svelte projection profile.
 *
 * The modern Svelte Component is function-shaped and framework-specific; no
 * Vue constructor, Vue event/model/ref convention, or InstanceType-of-a-class
 * requirement applies. Runes semantics and module/instance script
 * participation are observed through the legal `.svelte.ts` module surface
 * imported below.
 */
import { bump, counter, derivedCount } from "./state-module.svelte";

export interface SvelteComponent<
  Props extends Record<string, unknown> = Record<string, never>,
  Exports extends Record<string, unknown> = Record<string, never>,
> {
  (this: void, internals: unknown, props: Props): Exports;
}

export type Item = { id: number; label: string };

export const Widget: SvelteComponent<{ item: Item }, { peek: () => Item }> = (
  _internals,
  props,
) => ({
  peek: () => props.item,
});

// Module-script (`.svelte.ts`) and instance-script participation share one
// program; runes reads stay typed end to end.
export const moduleObserved: number = counter.count;
export const derivedObserved: number = derivedCount;

/** STS0-hover */
export const sts0HoverTarget: number = bump(counter);

export const sts0DefinitionTarget: typeof Widget = Widget;
