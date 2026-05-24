// @ai-generated - Synthetic cross-file flow-return predicate fixture.

export const isNumber = (x: unknown): x is number => typeof x === "number";

export function isString(x: unknown): x is string {
  return typeof x === "string";
}

export function isText(x: unknown): x is string {
  return typeof x === "string";
}

export function assertUser(x: unknown): asserts x is { id: string } {
  if (!x || typeof (x as { id?: unknown }).id !== "string") throw new Error("missing id");
}

export function assertNumber(x: unknown): asserts x is number {
  if (typeof x !== "number") throw new Error("not number");
}
