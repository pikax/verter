// @ts-nocheck
export const counter = $state({ count: 0 });
export const derivedCount = $derived(counter.count + 1);
export function bump(cell) {
  return cell.count + 1;
}
