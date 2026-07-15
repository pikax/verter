// @ai-generated - Synthetic native function-body flow return catalog fixture.

export function bl01() {
  return 1;
}
export type BL01 = ReturnType<typeof bl01>;

export function bl02() {
  return { kind: "ok" as const, count: 1 };
}
export type BL02 = ReturnType<typeof bl02>;

export function bl03(flag: boolean) {
  if (flag) return "a" as const;
  return 1 as const;
}
export type BL03 = ReturnType<typeof bl03>;

export function bl04(flag: boolean) {
  if (flag) return 1 as const;
}
export type BL04 = ReturnType<typeof bl04>;

export function bl05(): never {
  throw new Error("x");
}
export type BL05 = ReturnType<typeof bl05>;

export function bl06(flag: boolean) {
  if (flag) throw new Error("x");
  return 1;
}
export type BL06 = ReturnType<typeof bl06>;

export async function bl07() {
  return 1;
}
export type BL07 = ReturnType<typeof bl07>;

export function* bl08() {
  yield 1;
  return "done" as const;
}
export type BL08 = ReturnType<typeof bl08>;

export function bl09() {
  return [1, "x"] as const;
}
export type BL09 = ReturnType<typeof bl09>;

export function bl10() {
  return [1, "x"];
}
export type BL10 = ReturnType<typeof bl10>;

export async function* bl11() {
  yield 1 as const;
  return "done" as const;
}
export type BL11 = ReturnType<typeof bl11>;

export function bl12() {
  return;
}
export type BL12 = ReturnType<typeof bl12>;

export function bl13() {
  throw new Error("x");
}
export type BL13 = ReturnType<typeof bl13>;

export function bl14() {
  if (false) return 1 as const;
  return "x" as const;
}
export type BL14 = ReturnType<typeof bl14>;

export function bl15() {
  while (true) {}
}
export type BL15 = ReturnType<typeof bl15>;

export function lr01(x: string) {
  return x;
}
export type LR01 = ReturnType<typeof lr01>;

export function lr02() {
  const x = "a" as const;
  return x;
}
export type LR02 = ReturnType<typeof lr02>;

export function lr03(x: string | number) {
  let y = x;
  if (typeof y === "string") return y;
  return y;
}
export type LR03 = ReturnType<typeof lr03>;

export function lr04(x: string | number) {
  let y = x;
  y = 1;
  return y;
}
export type LR04 = ReturnType<typeof lr04>;

export function lr05(flag: boolean) {
  let x = "a";
  if (flag) x = "b";
  return x;
}
export type LR05 = ReturnType<typeof lr05>;

export function lr06(input: { value: string | number }) {
  const { value } = input;
  if (typeof value === "string") return value;
  return value;
}
export type LR06 = ReturnType<typeof lr06>;

export function lr07(x: { a: { b: string } }) {
  return x.a.b;
}
export type LR07 = ReturnType<typeof lr07>;

export function lr08(x?: { a?: { b: string } }) {
  return x?.a?.b;
}
export type LR08 = ReturnType<typeof lr08>;

export function lr09(x: string | undefined) {
  return x!;
}
export type LR09 = ReturnType<typeof lr09>;

export function lr10(cb: (set: () => void) => void) {
  let x: string | number = "a";
  cb(() => {
    x = 1;
  });
  return x;
}
export type LR10 = ReturnType<typeof lr10>;

export function lr11(flag: boolean) {
  if (flag) var x = 1;
  return x;
}
export type LR11 = ReturnType<typeof lr11>;

export function lr12(x: { state: "idle" | "ready" }) {
  x.state = "ready";
  return x.state;
}
export type LR12 = ReturnType<typeof lr12>;

export function cn01(x: string | number) {
  if (typeof x === "string") return x;
  return x;
}
export type CN01 = ReturnType<typeof cn01>;

export function cn02(x: "" | "a" | 0 | 1 | null | undefined) {
  if (x) return x;
  return "fallback" as const;
}
export type CN02 = ReturnType<typeof cn02>;

export function cn03(x?: { a: string } | null) {
  if (x != null) return x.a;
  return "none" as const;
}
export type CN03 = ReturnType<typeof cn03>;

export function cn04(x: string | undefined) {
  if (x === undefined) return "missing" as const;
  return x;
}
export type CN04 = ReturnType<typeof cn04>;

export function cn05(x: "a" | "b" | "c") {
  if (x === "a") return 1 as const;
  return x;
}
export type CN05 = ReturnType<typeof cn05>;

export type CN06A = { kind: "a"; a: string };
export type CN06B = { kind: "b"; b: number };
export function cn06(x: CN06A | CN06B) {
  switch (x.kind) {
    case "a":
      return x.a;
    case "b":
      return x.b;
  }
}
export type CN06 = ReturnType<typeof cn06>;

export function cn07(x: { a: string } | { b: number }) {
  if ("a" in x) return x.a;
  return x.b;
}
export type CN07 = ReturnType<typeof cn07>;

export function cn08(x: string[] | string) {
  if (Array.isArray(x)) return x[0];
  return x;
}
export type CN08 = ReturnType<typeof cn08>;

export class CN09A {
  a = 1 as const;
}
export class CN09B {
  b = "x" as const;
}
export function cn09(x: CN09A | CN09B) {
  if (x instanceof CN09A) return x.a;
  return x.b;
}
export type CN09 = ReturnType<typeof cn09>;

export function cn10(x: string | number | undefined, flag: boolean) {
  if ((typeof x === "string" && flag) || typeof x === "number") return x;
  return undefined;
}
export type CN10 = ReturnType<typeof cn10>;

export function cn11(x: string | number | boolean) {
  if (typeof x !== "string") return x;
  return x.toUpperCase();
}
export type CN11 = ReturnType<typeof cn11>;

export type CN12Shape = { kind: "circle"; radius: number } | { kind: "square"; side: number };
export function cn12(s: CN12Shape) {
  if (s.kind === "circle") return s.radius;
  if (s.kind === "square") return s.side;
  return s;
}
export type CN12 = ReturnType<typeof cn12>;

export function cn13(x: { label?: string }) {
  if (x.label) return x.label;
  return "none" as const;
}
export type CN13 = ReturnType<typeof cn13>;

export function cn14(x: "a" | "b", y: "b" | "c") {
  if (x === y) return x;
  return y;
}
export type CN14 = ReturnType<typeof cn14>;

export type CN15A = { meta: { kind: "a" }; value: string };
export type CN15B = { meta: { kind: "b" }; value: number };
export function cn15(x: CN15A | CN15B) {
  if (x.meta.kind === "a") return x.value;
  return x.value;
}
export type CN15 = ReturnType<typeof cn15>;

export type CN16A = { kind: "a"; value: string };
export type CN16B = { kind: "b"; value: number };
export function cn16(x: CN16A | CN16B) {
  const { kind } = x;
  if (kind === "a") return x.value;
  return x.value;
}
export type CN16 = ReturnType<typeof cn16>;

export type PA01Fish = { swim(): string };
export type PA01Bird = { fly(): number };
export function pa01IsFish(x: PA01Fish | PA01Bird): x is PA01Fish {
  return "swim" in x;
}
export function pa01(x: PA01Fish | PA01Bird) {
  if (pa01IsFish(x)) return x.swim();
  return x.fly();
}
export type PA01 = ReturnType<typeof pa01>;

export function pa02AssertString(x: unknown): asserts x is string {
  if (typeof x !== "string") throw new Error("not string");
}
export function pa02(x: unknown) {
  pa02AssertString(x);
  return x;
}
export type PA02 = ReturnType<typeof pa02>;

export function pa03Assert(condition: unknown): asserts condition {
  if (!condition) throw new Error("failed");
}
export function pa03(x: string | undefined) {
  pa03Assert(x);
  return x;
}
export type PA03 = ReturnType<typeof pa03>;

export function pa04IsDefined<T>(x: T): x is NonNullable<T> {
  return x != null;
}
export function pa04(x: string | undefined) {
  if (pa04IsDefined(x)) return x;
  return "fallback" as const;
}
export type PA04 = ReturnType<typeof pa04>;

export function pa05IsString(x: unknown): x is string {
  return typeof x === "string";
}
export function pa05(x: unknown) {
  if (pa05IsString(x)) return x;
  return "fallback" as const;
}
export type PA05 = ReturnType<typeof pa05>;

export type PA06A = { kind: "a"; value: string };
export type PA06B = { kind: "b"; value: number };
export function pa06IsA(x: PA06A | PA06B): x is PA06A {
  return x.kind === "a";
}
export function pa06HasValue(x: PA06A): x is PA06A & { value: string } {
  return x.value.length > 0;
}
export function pa06(x: PA06A | PA06B) {
  if (pa06IsA(x) && pa06HasValue(x)) return x.value;
  return 0 as const;
}
export type PA06 = ReturnType<typeof pa06>;

export function pa07HasName(x: { name?: unknown }): x is { name: string } {
  return typeof x.name === "string";
}
export function pa07(x: { name?: unknown }) {
  if (pa07HasName(x)) return x.name;
  return undefined;
}
export type PA07 = ReturnType<typeof pa07>;

export function pa08AssertNumber(x: unknown): asserts x is number {
  if (typeof x !== "number") throw new Error("not number");
}
export function pa08(x: unknown) {
  pa08AssertNumber(x);
  return x;
}
export type PA08 = ReturnType<typeof pa08>;

export declare function pa09IsUser(x: unknown): x is { id: string };
export function pa09(x: unknown) {
  if (pa09IsUser(x)) return x.id;
  return undefined;
}
export type PA09 = ReturnType<typeof pa09>;

export function cg01Make() {
  return { a: 1 as const };
}
export function cg01() {
  return cg01Make();
}
export type CG01 = ReturnType<typeof cg01>;

export function cg02Id<T>(x: T) {
  return x;
}
export function cg02() {
  return cg02Id({ a: 1 as const });
}
export type CG02 = ReturnType<typeof cg02>;

export function cg03Wrap<T>(x: T): { value: T } {
  return { value: x };
}
export function cg03() {
  return cg03Wrap("x" as const);
}
export type CG03 = ReturnType<typeof cg03>;

export function cg04Pick(x: string): string;
export function cg04Pick(x: number): number;
export function cg04Pick(x: string | number) {
  return x;
}
export function cg04() {
  return cg04Pick(1);
}
export type CG04 = ReturnType<typeof cg04>;

export function cg05First<T>(...items: T[]) {
  return items[0];
}
export function cg05() {
  return cg05First("a" as const, "b" as const);
}
export type CG05 = ReturnType<typeof cg05>;

export function cg06Label(x = "default") {
  return x;
}
export function cg06() {
  return cg06Label();
}
export type CG06 = ReturnType<typeof cg06>;

export function cg07UseValue<T>(getter: () => T): T {
  return getter();
}
export function cg07(flag: boolean) {
  return cg07UseValue(() => (flag ? ("a" as const) : (1 as const)));
}
export type CG07 = ReturnType<typeof cg07>;

export function cg08WithItem<T, R>(item: T, cb: (item: T) => R): R {
  return cb(item);
}
export function cg08() {
  return cg08WithItem({ id: "x" as const }, (item) => item.id);
}
export type CG08 = ReturnType<typeof cg08>;

export declare const cg09Value: { id: "a"; extra: number };
export function cg09GetId<T extends { id: string }>(x: T) {
  return x.id;
}
export function cg09() {
  return cg09GetId(cg09Value);
}
export type CG09 = ReturnType<typeof cg09>;

export function cg10(n: number) {
  if (n <= 0) return 0;
  return cg10(n - 1);
}
export type CG10 = ReturnType<typeof cg10>;

export class CG11User {
  id = "x" as const;
}
export function cg11() {
  return new CG11User();
}
export type CG11 = ReturnType<typeof cg11>;

export type HOComputedRef<T> = { readonly value: T };
export declare function ho01Computed<T>(getter: () => T): HOComputedRef<T>;
export function ho01(flag: boolean) {
  return ho01Computed(() => (flag ? ("on" as const) : (0 as const)));
}
export type HO01 = ReturnType<typeof ho01>;

export function ho02(xs: (string | number)[]) {
  return xs.filter((x): x is string => typeof x === "string");
}
export type HO02 = ReturnType<typeof ho02>;

export function ho03(xs: (string | number)[]) {
  return xs.map((x) => (typeof x === "string" ? x.length : x.toString()));
}
export type HO03 = ReturnType<typeof ho03>;

export function ho04(xs: number[]) {
  return xs.reduce((sum, x) => sum + x, 0);
}
export type HO04 = ReturnType<typeof ho04>;

export function ho05(xs: (string | number)[]) {
  return xs.flatMap((x) => (typeof x === "string" ? [x] : []));
}
export type HO05 = ReturnType<typeof ho05>;

export function ho06Select<T, R>(value: T, fn: (value: T) => R): R {
  return fn(value);
}
export function ho06(x: { id: string }) {
  return ho06Select(x, (value) => value.id);
}
export type HO06 = ReturnType<typeof ho06>;

export function ho07(groups: Array<Array<string | number>>) {
  return groups.map((group) => group.filter((x): x is string => typeof x === "string"));
}
export type HO07 = ReturnType<typeof ho07>;

export function ho08(xs: Array<{ kind: "a"; a: string } | { kind: "b"; b: number }>) {
  return xs.map((x) => (x.kind === "a" ? x.a : x.b));
}
export type HO08 = ReturnType<typeof ho08>;

export declare function ho09Mystery<T>(cb: () => T): unknown;
export function ho09() {
  return ho09Mystery(() => ({ a: 1 as const }));
}
export type HO09 = ReturnType<typeof ho09>;

export function ho10Make<T>(x: T) {
  return () => x;
}
export function ho10() {
  return ho10Make("x" as const);
}
export type HO10 = ReturnType<typeof ho10>;

export async function ho11(xs: string[]) {
  return Promise.all(xs.map(async (x) => x.length));
}
export type HO11 = ReturnType<typeof ho11>;

export function ob01(x: { value: string | number }) {
  if (typeof x.value === "string") return x.value;
  return x.value;
}
export type OB01 = ReturnType<typeof ob01>;

export const ob02Base = { a: 1 as const, b: "x" as const };
export function ob02() {
  return { ...ob02Base, b: "y" as const };
}
export type OB02 = ReturnType<typeof ob02>;

export function ob03(flag: boolean) {
  return { a: 1, ...(flag ? { b: "x" as const } : {}) };
}
export type OB03 = ReturnType<typeof ob03>;

export function ob04() {
  return { mode: "dark", nested: { count: 1 } } as const;
}
export type OB04 = ReturnType<typeof ob04>;

export type OB05Config = { mode: "dark" | "light"; debug: boolean };
export function ob05() {
  return { mode: "dark", debug: false } satisfies OB05Config;
}
export type OB05 = ReturnType<typeof ob05>;

export function ob06<T extends { [key: string]: number }>(x: T, key: keyof T) {
  return x[key];
}
export type OB06 = ReturnType<typeof ob06>;

export const ob07Key = "name" as const;
export function ob07() {
  return { [ob07Key]: "Ada" as const };
}
export type OB07 = ReturnType<typeof ob07>;

export function ob08(x: { readonly id: string }) {
  return x;
}
export type OB08 = ReturnType<typeof ob08>;

export function ob09Keys<T extends object>(x: T) {
  return Object.keys(x) as Array<keyof T>;
}
export function ob09() {
  return ob09Keys({ a: 1, b: 2 });
}
export type OB09 = ReturnType<typeof ob09>;

export function ob10CloneFlags<T extends string>(...keys: T[]): { [K in T]: boolean } {
  return Object.fromEntries(keys.map((k) => [k, true])) as { [K in T]: boolean };
}
export function ob10() {
  return ob10CloneFlags("a", "b");
}
export type OB10 = ReturnType<typeof ob10>;

export type OB11Box<T> = T extends string ? { text: T } : { value: T };
export function ob11Box<T>(x: T): OB11Box<T> {
  return (typeof x === "string" ? { text: x } : { value: x }) as OB11Box<T>;
}
export function ob11() {
  return ob11Box("x" as const);
}
export type OB11 = ReturnType<typeof ob11>;

export declare const ob12Key: unique symbol;
export function ob12() {
  return { [ob12Key]: 1 };
}
export type OB12 = ReturnType<typeof ob12>;

export function ob13() {
  return {
    get value() {
      return 1 as const;
    },
  };
}
export type OB13 = ReturnType<typeof ob13>;

export function cf01(x: string | number | undefined) {
  if (typeof x === "string" && x.length > 0) return x;
  if (typeof x === "number") return x;
  return undefined;
}
export type CF01 = ReturnType<typeof cf01>;

export function cf02(x: "a" | "b" | null) {
  return x && { value: x };
}
export type CF02 = ReturnType<typeof cf02>;

export function cf03(x: "" | "a", y: "fallback") {
  return x || y;
}
export type CF03 = ReturnType<typeof cf03>;

export function cf04(x: string | undefined, y: string) {
  return x ?? y;
}
export type CF04 = ReturnType<typeof cf04>;

export function cf05(x: string | undefined) {
  if (!x) return "none" as const;
  return x;
}
export type CF05 = ReturnType<typeof cf05>;

export function cf06() {
  try {
    return 1 as const;
  } catch {
    return "err" as const;
  } finally {
  }
}
export type CF06 = ReturnType<typeof cf06>;

export function cf07(xs: string[]) {
  for (const x of xs) {
    if (x.length > 0) return x;
    break;
  }
  return undefined;
}
export type CF07 = ReturnType<typeof cf07>;

export function cf08(xs: Array<string | undefined>) {
  let last: string | undefined;
  for (const x of xs) {
    if (!x) continue;
    last = x;
  }
  return last;
}
export type CF08 = ReturnType<typeof cf08>;

export function cf09(x: string | number) {
  while (typeof x === "string") {
    x = x.length;
  }
  return x;
}
export type CF09 = ReturnType<typeof cf09>;

export function cf10() {
  try {
    return 1 as const;
  } finally {
    return "final" as const;
  }
}
export type CF10 = ReturnType<typeof cf10>;

export function cf11(run: (cb: () => void) => void) {
  let x: string | number = "x";
  if (typeof x === "string") {
    run(() => {
      x = 1;
    });
    return x;
  }
  return x;
}
export type CF11 = ReturnType<typeof cf11>;

export declare function cf12Mutate(x: { value: string | number }): void;
export function cf12(x: { value: string | number }) {
  if (typeof x.value === "string") {
    cf12Mutate(x);
    return x.value;
  }
  return x.value;
}
export type CF12 = ReturnType<typeof cf12>;

export function cf13(xs: number[][]) {
  outer: for (const row of xs) {
    for (const x of row) {
      if (x > 0) {
        break outer;
      }
    }
  }
  return "done" as const;
}
export type CF13 = ReturnType<typeof cf13>;

export function cf14(x: "a" | "b" | "c") {
  switch (x) {
    case "a":
    case "b":
      return x;
    default:
      return 0 as const;
  }
}
export type CF14 = ReturnType<typeof cf14>;

export function cf15(flag: boolean) {
  let x: string;
  if (flag) x = "a";
  return x;
}
export type CF15 = ReturnType<typeof cf15>;

export function cf16(x: string | number | boolean) {
  for (let i = 0; i < 1000; i++) {
    if (typeof x === "string") x = x.length;
    else if (typeof x === "number") x = x > 0;
    else x = "done";
  }
  return x;
}
export type CF16 = ReturnType<typeof cf16>;

export type VV01ComputedRef<T> = { readonly value: T };
export declare function vv01Computed<T>(getter: () => T): VV01ComputedRef<T>;
export function vv01(flag: boolean) {
  return vv01Computed(() =>
    flag
      ? { kind: "ready" as const, value: "on" as const }
      : { kind: "empty" as const, value: 0 as const },
  );
}
export type VV01 = ReturnType<typeof vv01>;

export type VV02Ref<T> = { value: T };
export declare function vv02Ref<T>(value: T): VV02Ref<T>;
export function vv02() {
  return vv02Ref("idle");
}
export type VV02 = ReturnType<typeof vv02>;

export type VV03Ref<T> = { value: T };
export declare function vv03Unref<T>(x: VV03Ref<T>): T;
export declare function vv03Unref<T>(x: T): T;
export function vv03(x: string | VV03Ref<number>) {
  return typeof x === "string" ? vv03Unref(x) : vv03Unref(x);
}
export type VV03 = ReturnType<typeof vv03>;

export declare function vv04Reactive<T extends object>(x: T): T;
export function vv04() {
  const state = vv04Reactive({ value: undefined as string | undefined });
  if (state.value) return state.value;
  return "none" as const;
}
export type VV04 = ReturnType<typeof vv04>;

export function vv05CreateProps() {
  return {
    label: "" as string,
    disabled: false,
  };
}
export type VV05 = ReturnType<typeof vv05CreateProps>;

export type VV06Props = { items?: string[] };
export type VV06DefaultValue<T> = T | (() => T);
export type VV06Defaults<T> = {
  [K in keyof T]?: VV06DefaultValue<NonNullable<T[K]>>;
};
export declare function vv06WithDefaults<T, D extends VV06Defaults<T>>(
  props: T,
  defaults: D,
): T & { [K in Extract<keyof D, keyof T>]-?: NonNullable<T[K]> };
export declare function vv06DefineProps<T>(): T;
export const vv06Props = vv06WithDefaults(vv06DefineProps<VV06Props>(), {
  items: () => ["a", "b"],
});
export function vv06() {
  return vv06Props.items;
}
export type VV06 = ReturnType<typeof vv06>;

export type VV07Ref<T> = { value: T };
export declare function vv07Ref<T>(value: T): VV07Ref<T>;
export function vv07UseCounter() {
  const count = vv07Ref(0);
  const inc = () => {
    count.value += 1;
  };
  return { count, inc };
}
export type VV07 = ReturnType<typeof vv07UseCounter>;

export type VV08ComputedRef<T> = { readonly value: T };
export declare function vv08Computed<T>(getter: () => T): VV08ComputedRef<T>;
export type VV08Props = { kind: "text"; value: string } | { kind: "count"; value: number };
export declare const vv08Props: VV08Props;
export function vv08() {
  return vv08Computed(() => (vv08Props.kind === "text" ? vv08Props.value : vv08Props.value));
}
export type VV08 = ReturnType<typeof vv08>;

export type VV09Node = { __vnode: true };
export type VV09Slots = {
  default?: (props: { selected: boolean }) => VV09Node[];
};
export declare const vv09Slots: VV09Slots;
export function vv09() {
  return vv09Slots.default?.({ selected: true });
}
export type VV09 = ReturnType<typeof vv09>;

export function vv10Child() {
  return {
    focus() {
      return true as const;
    },
  };
}
export declare function vv10UseTemplateRef<T>(name: string): { value: T | null };
export function vv10() {
  const child = vv10UseTemplateRef<ReturnType<typeof vv10Child>>("child");
  return child.value?.focus();
}
export type VV10 = ReturnType<typeof vv10>;

export type VV11Emit = {
  (event: "save", payload: { id: string }): boolean;
};
export declare const vv11Emit: VV11Emit;
export function vv11(id: string) {
  return vv11Emit("save", { id });
}
export type VV11 = ReturnType<typeof vv11>;

export declare function vv12DefineModel<T>(opts: {
  get?: (value: T) => T;
  set?: (value: T) => T;
}): { value: T };
export const vv12Model = vv12DefineModel<string>({
  set(value) {
    return value.trim();
  },
});
export function vv12() {
  return vv12Model.value;
}
export type VV12 = ReturnType<typeof vv12>;

export declare function vv13Watch<T>(source: () => T, cb: (value: T) => void): void;
export function vv13(value: string | undefined) {
  let last = "none";
  vv13Watch(
    () => value,
    (v) => {
      if (v) last = v;
    },
  );
  return last;
}
export type VV13 = ReturnType<typeof vv13>;

export declare function vv14Inject<T>(key: string): T | undefined;
export function vv14AssertProvided<T>(value: T | undefined): asserts value is T {
  if (value === undefined) throw new Error("missing");
}
export function vv14() {
  const service = vv14Inject<{ id: string }>("service");
  vv14AssertProvided(service);
  return service.id;
}
export type VV14 = ReturnType<typeof vv14>;

export function vv15CreateDefaults() {
  return { size: "md" as const, disabled: false };
}
export type VV15 = ReturnType<typeof vv15CreateDefaults>;

export function vv16A() {
  return { kind: "A" as const };
}
export function vv16B() {
  return { kind: "B" as const };
}
export function vv16Resolve(flag: boolean) {
  return flag ? vv16A : vv16B;
}
export function vv16(flag: boolean) {
  const Comp = vv16Resolve(flag);
  return Comp();
}
export type VV16 = ReturnType<typeof vv16>;

export function bl16(): { tag: "ready"; count: number } {
  return { tag: "ready", count: 1 as const };
}
export type BL16 = ReturnType<typeof bl16>;

export async function bl17() {
  return Promise.resolve({ id: "ok" as const });
}
export type BL17 = ReturnType<typeof bl17>;

export function bl18(flag: boolean) {
  if (flag) return;
  return "ready" as const;
}
export type BL18 = ReturnType<typeof bl18>;

export type CN17A = { meta?: { kind: "a"; value: string } };
export type CN17B = { meta?: { kind: "b"; value: number } };
export function cn17(x: CN17A | CN17B) {
  if (x.meta?.kind === "a") return x.meta.value;
  return 0 as const;
}
export type CN17 = ReturnType<typeof cn17>;

export function cn18(x: { a: string } | { b: number }) {
  if (!("a" in x)) return x.b;
  return x.a;
}
export type CN18 = ReturnType<typeof cn18>;

export type CN19A = { kind: "a"; payload: { label: string } };
export type CN19B = { kind: "b"; payload: { count: number } };
export function cn19(x: CN19A | CN19B) {
  if (x.kind === "a" && x.payload.label) return x.payload.label;
  if (x.kind === "b") return x.payload.count;
  return undefined;
}
export type CN19 = ReturnType<typeof cn19>;

export function cn20(x: string | number | boolean) {
  if (typeof x === "string") {
    return x.length;
  } else if (typeof x === "number") {
    return x.toString();
  }
  return x;
}
export type CN20 = ReturnType<typeof cn20>;

export function pa10AssertHasId(x: { id?: unknown }): asserts x is { id: string } {
  if (typeof x.id !== "string") throw new Error("missing id");
}
export function pa10(x: { id?: unknown }) {
  pa10AssertHasId(x);
  return x.id;
}
export type PA10 = ReturnType<typeof pa10>;

export declare function pa11AssertArray<T>(x: T | T[]): asserts x is T[];
export function pa11(x: string | string[]) {
  pa11AssertArray(x);
  return x[0];
}
export type PA11 = ReturnType<typeof pa11>;

export type PA12Box<T> = { value: T | undefined };
export function pa12HasValue<T>(box: PA12Box<T>): box is { value: T } {
  return box.value !== undefined;
}
export function pa12(box: PA12Box<string>) {
  if (pa12HasValue(box)) return box.value;
  return "fallback" as const;
}
export type PA12 = ReturnType<typeof pa12>;

export function cg12Tuple<T extends readonly unknown[]>(...items: T): T {
  return items;
}
export function cg12() {
  return cg12Tuple("a" as const, 1 as const)[1];
}
export type CG12 = ReturnType<typeof cg12>;

export function cg13MakeGetter<T>(value: T) {
  return () => value;
}
export function cg13() {
  return cg13MakeGetter({ id: "x" as const })().id;
}
export type CG13 = ReturnType<typeof cg13>;

export function cg14Apply<T, R>(value: T, fn: (this: T) => R): R {
  return fn.call(value);
}
export function cg14() {
  return cg14Apply({ id: "x" as const }, function () {
    return this.id;
  });
}
export type CG14 = ReturnType<typeof cg14>;

export type CG15Result<T> = T extends "a" ? { a: string } : { b: number };
export function cg15Pick<T extends "a" | "b">(key: T): CG15Result<T> {
  return (key === "a" ? { a: "" } : { b: 1 }) as CG15Result<T>;
}
export function cg15() {
  return cg15Pick("b").b;
}
export type CG15 = ReturnType<typeof cg15>;

export function cg16Default<T extends { id: string } = { id: "default" }>(): T {
  return { id: "default" } as T;
}
export function cg16() {
  return cg16Default().id;
}
export type CG16 = ReturnType<typeof cg16>;

export function ho12(xs: (string | number)[]) {
  return xs.find((x): x is string => typeof x === "string");
}
export type HO12 = ReturnType<typeof ho12>;

export function ho13(xs: (string | number)[]) {
  if (xs.every((x): x is string => typeof x === "string")) return xs[0];
  return 0 as const;
}
export type HO13 = ReturnType<typeof ho13>;

export function ho14(keys: string[]) {
  return keys.reduce<Record<string, boolean>>((acc, key) => {
    acc[key] = true;
    return acc;
  }, {});
}
export type HO14 = ReturnType<typeof ho14>;

export function ho15(p: Promise<string>) {
  return p.then((value) => value.length);
}
export type HO15 = ReturnType<typeof ho15>;

export function ob14() {
  return {
    run() {
      return "ok" as const;
    },
  };
}
export type OB14 = ReturnType<typeof ob14>;

export const ob15Prefix = "field" as const;
export function ob15() {
  return { [`${ob15Prefix}Name`]: "Ada" as const };
}
export type OB15 = ReturnType<typeof ob15>;

export function ob16() {
  return { items: [{ id: "a" as const }] } as const;
}
export type OB16 = ReturnType<typeof ob16>;

export function ob17(flag: boolean) {
  return {
    base: true as const,
    ...(flag ? { enabled: "yes" as const } : { disabled: "no" as const }),
  };
}
export type OB17 = ReturnType<typeof ob17>;

export type OB18Shape = { nested: { label: string }; count: number };
export function ob18() {
  return { nested: { label: "x" }, count: 1 } satisfies OB18Shape;
}
export type OB18 = ReturnType<typeof ob18>;

export type VV17AsyncComputedRef<T> = { readonly value: Promise<T> };
export declare function vv17AsyncComputed<T>(getter: () => Promise<T>): VV17AsyncComputedRef<T>;
export function vv17(flag: boolean) {
  return vv17AsyncComputed(async () => (flag ? ("yes" as const) : (0 as const)));
}
export type VV17 = ReturnType<typeof vv17>;

export type VV18ModelOptions<T> = {
  get?: (value: T | undefined) => T;
  set?: (value: T) => T | undefined;
};
export declare function vv18Model<T>(opts: VV18ModelOptions<T>): { value: T | undefined };
export const vv18State = vv18Model<string>({
  get(value) {
    return value ?? "fallback";
  },
});
export function vv18() {
  return vv18State.value;
}
export type VV18 = ReturnType<typeof vv18>;

export declare function vv19MaybeSlot(): ((value: string) => { node: true }) | undefined;
export function vv19() {
  return vv19MaybeSlot()?.("ok");
}
export type VV19 = ReturnType<typeof vv19>;

export declare function vv20UseService<T>(): { current: T | null };
export function vv20() {
  const service = vv20UseService<{ run(): "done" }>();
  return service.current?.run();
}
export type VV20 = ReturnType<typeof vv20>;

export const bl19 = () => ({ tag: "arrow" as const, count: 1 });
export type BL19 = ReturnType<typeof bl19>;

export const bl20: () => { id: string; ready: boolean } = function () {
  return { id: "literal" as const, ready: true as const };
};
export type BL20 = ReturnType<typeof bl20>;

export async function bl21(input: Promise<{ id: string }>) {
  return await input;
}
export type BL21 = ReturnType<typeof bl21>;

export function* bl22(values: Iterable<1 | 2>) {
  yield* values;
  return "done" as const;
}
export type BL22 = ReturnType<typeof bl22>;

export function bl23(): void {
  return "ignored" as never;
}
export type BL23 = ReturnType<typeof bl23>;
