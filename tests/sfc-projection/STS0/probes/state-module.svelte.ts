/**
 * Legal `.svelte.ts` module surface. Rune declarations match the CCA1I
 * projection prelude and the pinned Svelte 5.56.10 types
 * (`$derived<T>(expression: T): T`; callback evaluation is `$derived.by`).
 */
declare function $state<T>(initial: T): T;
declare function $derived<T>(expression: T): T;
declare namespace $derived {
  function by<T>(fn: () => T): T;
}

export interface CounterCell {
  count: number;
}

export const counter: CounterCell = $state({ count: 0 });

export const derivedCount: number = $derived(counter.count + 1);

export function bump(cell: CounterCell): number {
  return cell.count + 1;
}
