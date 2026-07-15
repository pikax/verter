// @ai-generated - Synthetic TypeScript parity gap contracts for flow-return tests.

export class TP01PrivateBox {
  #value: string | undefined;

  constructor(value?: string) {
    this.#value = value;
  }

  read() {
    if (this.#value) return this.#value;
    return "fallback" as const;
  }
}
export type TP01 = ReturnType<TP01PrivateBox["read"]>;

export class TP02Base {
  protected value: string | number;

  constructor(value: string | number) {
    this.value = value;
  }

  read() {
    if (typeof this.value === "string") return this.value;
    return this.value;
  }
}
export type TP02 = ReturnType<TP02Base["read"]>;

export class TP03Accessors {
  #count = 0;

  get value() {
    return this.#count === 0 ? ("empty" as const) : this.#count;
  }

  set value(value: number | "empty") {
    this.#count = value === "empty" ? 0 : value;
  }
}
export function tp03(instance: TP03Accessors) {
  return instance.value;
}
export type TP03 = ReturnType<typeof tp03>;

export async function* tp04(source: AsyncIterable<string | number>) {
  for await (const value of source) {
    if (typeof value === "string") yield value.length;
    else yield value;
  }
  return "done" as const;
}
export type TP04 = ReturnType<typeof tp04>;

export function tp05Select<T extends { kind: "left"; value: string }>(
  input: T,
  pick: (input: T) => "left",
): string;
export function tp05Select<T extends { kind: "right"; value: number }>(
  input: T,
  pick: (input: T) => "right",
): number;
export function tp05Select(
  input: { kind: "left"; value: string } | { kind: "right"; value: number },
  pick: (
    input: { kind: "left"; value: string } | { kind: "right"; value: number },
  ) => "left" | "right",
) {
  return pick(input) === "left" ? String(input.value) : Number(input.value);
}
export function tp05() {
  return tp05Select({ kind: "right" as const, value: 1 }, (input) => input.kind);
}
export type TP05 = ReturnType<typeof tp05>;

export type TP06Node<T> = {
  tag: string;
  props: T;
};
export function tp06JsxLike<T>(tag: string, props: T): TP06Node<T> {
  return { tag, props };
}
export function tp06(flag: boolean) {
  const node = flag
    ? tp06JsxLike("item", { kind: "text" as const, value: "x" })
    : tp06JsxLike("item", { kind: "count" as const, value: 1 });
  return node.props.value;
}
export type TP06 = ReturnType<typeof tp06>;

export type TP07Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function tp07(shape: TP07Shape) {
  switch (shape.kind) {
    case "a":
      return shape.a;
    case "b":
      return shape.b;
    default:
      return shape satisfies never;
  }
}
export type TP07 = ReturnType<typeof tp07>;

export type TP08Disposer = {
  disposed: boolean;
  dispose(): void;
};
export function tp08(use: (disposer: TP08Disposer) => string | number) {
  const disposer: TP08Disposer = {
    disposed: false,
    dispose() {
      this.disposed = true;
    },
  };
  try {
    return use(disposer);
  } finally {
    disposer.dispose();
  }
}
export type TP08 = ReturnType<typeof tp08>;

export function tp09<T extends readonly unknown[]>(
  values: T,
): T extends readonly [infer First, ...unknown[]] ? First : undefined {
  return values[0] as T extends readonly [infer First, ...unknown[]] ? First : undefined;
}
export function tp09Case() {
  return tp09(["first", 1] as const);
}
export type TP09 = ReturnType<typeof tp09Case>;

export type TPKitchenInput =
  | {
      kind: "class";
      box: TP01PrivateBox;
      labels?: string[];
    }
  | {
      kind: "node";
      value: TP06Node<{ value: number }>;
    };

export function tpKitchen(input: TPKitchenInput) {
  if (input.kind === "class") {
    const found = input.labels?.find((label): label is string => label.length > 0);
    return {
      kind: "class" as const,
      value: input.box.read(),
      found,
    };
  }

  return {
    kind: "node" as const,
    value: input.value.props.value,
  };
}
export type TPKitchen = ReturnType<typeof tpKitchen>;
