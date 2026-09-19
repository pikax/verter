/**
 * Legal `.svelte.ts` module surface: runes participate in module-context
 * files outside the component file. The projection's ambient prelude types
 * the runes; for this stock-engine probe world the used runes are declared
 * in-module so both pinned engines observe the same typed surface.
 */
declare function $state<T>(initial: T): T;
declare function $derived<T>(compute: () => T): T;

export interface CounterCell {
  count: number;
}

export const counter: CounterCell = $state({ count: 0 });

export const derivedCount: number = $derived(() => counter.count + 1);

export function bump(cell: CounterCell): number {
  return cell.count + 1;
}
