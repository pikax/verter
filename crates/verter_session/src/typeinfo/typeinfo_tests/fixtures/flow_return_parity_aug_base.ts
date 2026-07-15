// @ai-generated - Synthetic module augmentation base for flow-return tests.

export interface ParityRegistry {
  base: string;
}

export function makeRegistry(): ParityRegistry {
  return {
    base: "base",
    extra: 1,
    nested: {
      label: "label",
    },
  } as ParityRegistry;
}
