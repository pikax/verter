// @ai-generated - Synthetic value-level type inference fixture.

export const literalConst = "ready";
export const numberConst = 42;
export let mutableLabel = "draft";
export var mutableCount = 1;

export const objectConst = {
  id: "item",
  nested: {
    value: 7,
    flag: true,
  },
  list: [1, 2, 3],
} as const;

export const derivedValue = objectConst.nested.value;

export const directArrow = (input: string, count?: number) => ({
  input,
  count,
  ok: true,
});

export function bodyReturn(flag: boolean) {
  if (flag) {
    return {
      state: "on" as const,
      value: 1,
    };
  }
  return {
    state: "off" as const,
    value: 0,
  };
}

export function flowReturn(input: string | number) {
  let current = input;
  if (typeof current === "string") {
    return {
      kind: "text" as const,
      value: current,
    };
  }
  return {
    kind: "number" as const,
    value: current,
  };
}

export type ComputedRef<T> = {
  readonly value: T;
};

export function computed<T>(getter: () => T): ComputedRef<T> {
  return {
    value: getter(),
  };
}

export const computedObject = computed(() => ({
  id: "computed" as const,
  count: 2,
  nested: {
    ready: true,
  },
}));

export const computedBlock = computed(() => {
  const local = {
    ready: true as const,
    count: 3,
  };
  return {
    state: local.ready,
    count: local.count,
  };
});

export type LiteralConstType = typeof literalConst;
export type NumberConstType = typeof numberConst;
export type MutableLabelType = typeof mutableLabel;
export type MutableCountType = typeof mutableCount;
export type ObjectConstType = typeof objectConst;
export type ObjectNestedType = typeof objectConst.nested;
export type DerivedValueType = typeof derivedValue;
export type DirectArrowReturn = ReturnType<typeof directArrow>;
export type BodyReturnType = ReturnType<typeof bodyReturn>;
export type FlowReturnType = ReturnType<typeof flowReturn>;
export type ComputedObjectType = typeof computedObject;
export type ComputedObjectValue = typeof computedObject.value;
export type ComputedBlockValue = typeof computedBlock.value;
