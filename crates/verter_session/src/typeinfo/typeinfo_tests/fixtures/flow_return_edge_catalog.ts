// @ai-generated - Synthetic edge contracts for native function-body flow returns.

export function lr13(input: { item?: { id?: string } } = {}) {
  const { item: { id = "missing" } = {} } = input;
  return id;
}
export type LR13 = ReturnType<typeof lr13>;

export function lr14(input: { keep: string; drop: number; extra?: boolean }) {
  const { drop, ...rest } = input;
  void drop;
  return rest;
}
export type LR14 = ReturnType<typeof lr14>;

export function lr15(input: readonly [string, number?]) {
  const [first, second = 0] = input;
  return [first, second] as const;
}
export type LR15 = ReturnType<typeof lr15>;

export function lr16() {
  let value: string | undefined;
  value ??= "set";
  return value;
}
export type LR16 = ReturnType<typeof lr16>;

export type CN21A = { kind: "a"; nested?: { value: string } };
export type CN21B = { kind: "b"; nested?: { value: number } };
export function cn21(input: CN21A | CN21B) {
  if (input.kind === "a" && input.nested?.value) return input.nested.value;
  return 0 as const;
}
export type CN21 = ReturnType<typeof cn21>;

export type CN22 = ReturnType<typeof cn22>;
export function cn22(input: ["text", string] | ["count", number]) {
  if (input[0] === "text") return input[1];
  return input[1];
}

export type CN23State = { status: "ready"; value: string } | { status: "idle"; value?: never };
export function cn23(input: CN23State) {
  if (input.status !== "ready") return undefined;
  return input.value;
}
export type CN23 = ReturnType<typeof cn23>;

export function cn24(input: unknown) {
  if (typeof input === "object" && input !== null && "id" in input) {
    return input.id;
  }
  return undefined;
}
export type CN24 = ReturnType<typeof cn24>;

export type PA13Box = {
  value?: string;
  hasValue(this: PA13Box): this is PA13Box & { value: string };
};
export function pa13(box: PA13Box) {
  if (box.hasValue()) return box.value;
  return undefined;
}
export type PA13 = ReturnType<typeof pa13>;

export function pa14AssertPresent<T>(value: T | undefined): asserts value is T {
  if (value === undefined) throw new Error("missing");
}
export function pa14(config: { value?: { id: string } }) {
  pa14AssertPresent(config.value);
  return config.value.id;
}
export type PA14 = ReturnType<typeof pa14>;

export function pa15HasKey<K extends PropertyKey>(
  input: object,
  key: K,
): input is Record<K, unknown> {
  return key in input;
}
export function pa15(input: object) {
  if (pa15HasKey(input, "name") && typeof input.name === "string") {
    return input.name;
  }
  return "";
}
export type PA15 = ReturnType<typeof pa15>;

export function pa16AssertNever(value: never): never {
  throw new Error(`unexpected ${value}`);
}
export type PA16Shape = { kind: "x"; x: string } | { kind: "y"; y: number };
export function pa16(shape: PA16Shape) {
  switch (shape.kind) {
    case "x":
      return shape.x;
    case "y":
      return shape.y;
    default:
      return pa16AssertNever(shape);
  }
}
export type PA16 = ReturnType<typeof pa16>;

export interface CG17Runner {
  (kind: "left"): { side: "left"; value: string };
  (kind: "right"): { side: "right"; value: number };
}
export declare const cg17Runner: CG17Runner;
export function cg17() {
  return cg17Runner("right").value;
}
export type CG17 = ReturnType<typeof cg17>;

export const cg18Identity = <T>(value: T) => value;
export function cg18() {
  return cg18Identity({ label: "ok" as const }).label;
}
export type CG18 = ReturnType<typeof cg18>;

export function cg19Get<T, K extends keyof T>(input: T, key: K): T[K] {
  return input[key];
}
export function cg19() {
  return cg19Get({ count: 1, label: "x" as const }, "label");
}
export type CG19 = ReturnType<typeof cg19>;

export class CG20Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
export function cg20() {
  return new CG20Box("boxed" as const).value;
}
export type CG20 = ReturnType<typeof cg20>;

export function cg21Run<T>(fn?: () => T) {
  return fn?.();
}
export function cg21() {
  return cg21Run(() => "ok" as const);
}
export type CG21 = ReturnType<typeof cg21>;

export function ho16(values: readonly (string | number)[]) {
  return values.find((value): value is string => typeof value === "string")?.toUpperCase();
}
export type HO16 = ReturnType<typeof ho16>;

export function ho17(entries: Array<[string, number]>) {
  return Object.fromEntries(
    entries.map(([key, value]) => [key, value.toString()] as const),
  ) as Record<string, string>;
}
export type HO17 = ReturnType<typeof ho17>;

export function ho18(label: Promise<string>, count: Promise<number>) {
  return Promise.all([label, count] as const).then(([resolvedLabel, resolvedCount]) => ({
    label: resolvedLabel,
    count: resolvedCount,
  }));
}
export type HO18 = ReturnType<typeof ho18>;

export function ho19(values: Array<string | number | boolean>) {
  return values.flatMap((value) => (typeof value === "string" ? [{ value }] : []));
}
export type HO19 = ReturnType<typeof ho19>;

export function ob19() {
  return Object.assign({ a: 1 as const }, { b: "x" as const });
}
export type OB19 = ReturnType<typeof ob19>;

export function ob20(flag: boolean) {
  return {
    maybe: flag ? () => "yes" as const : undefined,
  };
}
export type OB20 = ReturnType<typeof ob20>;

export function ob21<T extends { readonly id: string }>(input: T) {
  return { ...input, extra: true as const };
}
export function ob21Case() {
  return ob21({ id: "edge" as const });
}
export type OB21 = ReturnType<typeof ob21Case>;

export type OB22Shape = Record<"one" | "two", number>;
export function ob22() {
  return { one: 1, two: 2 } satisfies OB22Shape;
}
export type OB22 = ReturnType<typeof ob22>;

export function cf17(input: string | number) {
  do {
    if (typeof input === "string") return input;
    input = "done";
  } while (false);
  return input;
}
export type CF17 = ReturnType<typeof cf17>;

export type CF18Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function cf18(input: CF18Shape) {
  switch (input.kind) {
    case "a":
      return input.a;
    case "b":
      return input.b;
    default:
      throw new Error("unreachable");
  }
}
export type CF18 = ReturnType<typeof cf18>;

export function cf19(flag: boolean) {
  try {
    if (flag) return "ok" as const;
    throw "fail";
  } catch (error) {
    return error;
  }
}
export type CF19 = ReturnType<typeof cf19>;

export function cf20(input: Record<string, number>) {
  for (const key in input) return key;
  return undefined;
}
export type CF20 = ReturnType<typeof cf20>;

export function cf21(input: string | null) {
  if (input === null) return;
  return input;
}
export type CF21 = ReturnType<typeof cf21>;

export declare function cf22Read(): string;
export declare function cf22Cleanup(): void;
export function cf22() {
  try {
    return cf22Read();
  } finally {
    cf22Cleanup();
  }
}
export type CF22 = ReturnType<typeof cf22>;

export type VV21WritableRef<T> = { value: T };
export declare function vv21Computed<T>(options: {
  get(): T;
  set(value: T): void;
}): VV21WritableRef<T>;
export function vv21() {
  return vv21Computed({
    get() {
      return "ready" as const;
    },
    set(value) {
      value.toUpperCase();
    },
  }).value;
}
export type VV21 = ReturnType<typeof vv21>;

export declare function vv22DefineProps<T>(): T;
export function vv22() {
  const { label = "fallback" } = vv22DefineProps<{ label?: string }>();
  return label;
}
export type VV22 = ReturnType<typeof vv22>;

export type VV23Node = { node: string };
export type VV23Slots = Record<
  `cell:${string}`,
  ((ctx: { value: string }) => VV23Node) | undefined
>;
export declare const vv23Slots: VV23Slots;
export function vv23(key: `cell:${string}`) {
  return vv23Slots[key]?.({ value: "x" });
}
export type VV23 = ReturnType<typeof vv23>;

export declare function vv24Resolve<T>(source: () => T): { readonly value: T };
export function vv24(input: { kind: "id"; id: string } | { kind: "count"; count: number }) {
  return vv24Resolve(() => (input.kind === "id" ? input.id : input.count)).value;
}
export type VV24 = ReturnType<typeof vv24>;
