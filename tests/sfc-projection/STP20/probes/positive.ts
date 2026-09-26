type ChildProps =
  | { title: string; kind: "number"; value: number; optional?: string }
  | { title: string; kind: "text"; value: string; optional?: string };

interface ChildInstance {
  readonly $props: ChildProps;
}

declare const Child: { new (props: ChildProps): ChildInstance };

declare function __VerterUseConstructor<P, I>(
  component: abstract new (props: P) => I,
): new (props: P) => I;
type __VerterUseComponentProps<C> = C extends abstract new (props: infer P) => unknown ? P : never;
type __VerterUseKnownSpread<S, P, O extends PropertyKey> = string extends keyof S
  ? S
  : Exclude<keyof S, keyof P | O> extends never
    ? Omit<S, O> extends Partial<Omit<P, O>>
      ? S
      : never
    : never;
declare function __VerterUseSpread<C, S, O extends PropertyKey>(
  component: C,
  spread: S & __VerterUseKnownSpread<S, __VerterUseComponentProps<C>, O>,
  overwritten: readonly O[],
): S;

export type Instance = InstanceType<typeof Child>;
export const stp20DefinitionTarget: typeof Child = Child;

// A defaulted setup prop is optional at the caller. Setup itself reads the
// default-resolved string, not an optional value.
type DefaultedCaller = { title?: string };
declare const defaulted: DefaultedCaller;
export const callerMayOmitDefault: DefaultedCaller = defaulted;
export const setupDefault: string = defaulted.title ?? "fallback";

// Required props and matched discriminated pairs remain ordinary constructor
// obligations; omitted optional fields are distinct from `undefined`.
export const matched = new (__VerterUseConstructor(Child))({
  title: "ok",
  kind: "number",
  value: 1,
});
export const omittedOptional = new (__VerterUseConstructor(Child))({
  title: "ok",
  kind: "text",
  value: "x",
});
export const stp20HoverTarget: number = matched.$props.kind === "number" ? matched.$props.value : 0;

const spread = { title: "old", kind: "number", value: 1 } as const;
// The stale title is ignored because the later definite value is the one the
// child receives. Other finite keys still participate in the check.
export const overwritten = new (__VerterUseConstructor(Child))({
  ...__VerterUseSpread(Child, spread, ["title"] as const),
  title: "new",
});

declare const open: Record<string, unknown>;
export const openDomain = __VerterUseSpread(Child, open, [] as const);
