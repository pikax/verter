// @ai-generated - Synthetic module augmentation patch for flow-return tests.

import type { ParityRegistry } from "./flow_return_parity_aug_base";
import "./flow_return_parity_aug_base";

declare module "./flow_return_parity_aug_base" {
  interface ParityRegistry {
    extra: number;
    nested?: {
      label: string;
    };
  }
}

export function assertRegistry(input: unknown): asserts input is ParityRegistry {
  if (typeof input !== "object" || input === null || !("extra" in input)) {
    throw new Error("not a registry");
  }
}

export function mapRegistry<T>(registry: ParityRegistry, map: (registry: ParityRegistry) => T): T {
  return map(registry);
}
