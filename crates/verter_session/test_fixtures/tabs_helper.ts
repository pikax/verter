import type { TabItem } from "./tabs_types";

export function activeIndexFor(items: TabItem[], modelValue?: number): number {
  if (typeof modelValue === "number" && modelValue >= 0 && modelValue < items.length) {
    return modelValue;
  }
  for (let i = 0; i < items.length; i++) {
    if (!items[i].disabled) return i;
  }
  return 0;
}
