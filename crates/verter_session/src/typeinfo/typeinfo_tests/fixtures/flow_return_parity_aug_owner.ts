// @ai-generated - Synthetic module augmentation owner for flow-return tests.

import type { ParityRegistry } from "./flow_return_parity_aug_barrel";
import { assertRegistry, makeRegistry, mapRegistry } from "./flow_return_parity_aug_barrel";

export function mp01(input: ParityRegistry) {
  return input.extra;
}
export type MP01 = ReturnType<typeof mp01>;

export function mp02(input: unknown) {
  assertRegistry(input);
  return input.nested?.label;
}
export type MP02 = ReturnType<typeof mp02>;

export function mp03() {
  return mapRegistry(makeRegistry(), (registry) => ({
    extra: registry.extra,
    label: registry.nested?.label ?? registry.base,
  }));
}
export type MP03 = ReturnType<typeof mp03>;

export type MPKitchenInput =
  | {
      kind: "registry";
      value: ParityRegistry;
      labels?: string[];
    }
  | {
      kind: "fallback";
      value?: {
        id?: string;
      };
    };

export function mpKitchen(input: MPKitchenInput) {
  if (input.kind === "registry") {
    const first = input.labels?.find((label): label is string => label.length > 0);
    return mapRegistry(input.value, (registry) => ({
      kind: "registry" as const,
      extra: registry.extra,
      label: first ?? registry.nested?.label ?? registry.base,
    }));
  }

  return {
    kind: "fallback" as const,
    id: input.value?.id ?? "missing",
  };
}
export type MPKitchen = ReturnType<typeof mpKitchen>;
