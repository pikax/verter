// @ts-check
/**
 * @typedef {{ count: number }} CounterCell
 */

/** @type {CounterCell} */
export const counter = $state({ count: 0 });

/** @type {number} */
export const derivedCount = $derived(counter.count + 1);

/**
 * @param {CounterCell} cell
 * @returns {number}
 */
export function bump(cell) {
  return cell.count + 1;
}
