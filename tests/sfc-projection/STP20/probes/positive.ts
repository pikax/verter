type ChildProps =
  | { title: string; kind: "number"; value: number; optional?: string; tuple?: [string, number] }
  | { title: string; kind: "text"; value: string; optional?: string; tuple?: [string, number] };

interface ChildInstance {
  readonly $props: ChildProps;
}

declare const Child: { new (props: ChildProps): ChildInstance };

declare function __VerterUseConstructor<P, I>(
  component: abstract new (props: P) => I,
): new (props: P) => I;
type __VerterUseComponentProps<C> = C extends abstract new (props: infer P) => unknown ? P : never;
type __VerterUseResolvedKeys<S> = [keyof S] extends [infer K]
  ? [K] extends [PropertyKey]
    ? 1
    : 0
  : 0;
type __VerterUseMemberOpen<S> = S extends unknown
  ? string extends keyof S
    ? true
    : number extends keyof S
      ? true
      : symbol extends keyof S
        ? true
        : false
  : never;
type __VerterUseBranch<S, P> = [S] extends [P] ? true : false;
type __VerterUseChecked<S, P, O extends PropertyKey> = S extends unknown
  ? [true] extends [P extends unknown ? __VerterUseBranch<Omit<S, O>, Omit<P, O>> : never]
    ? true
    : false
  : never;
type __VerterUseDrop<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;
type __VerterUseKnownSpread<S, P, O extends PropertyKey> = [__VerterUseResolvedKeys<S>] extends [
  never,
]
  ? S
  : __VerterUseResolvedKeys<S> extends 1
    ? [__VerterUseMemberOpen<S>] extends [false]
      ? [__VerterUseChecked<S, P, O>] extends [true]
        ? S
        : S & __VerterUseDrop<P, O>
      : S
    : S;
declare function __VerterUseSpread<C, S, O extends PropertyKey>(
  component: C,
  spread: S & __VerterUseKnownSpread<S, __VerterUseComponentProps<C>, O>,
  overwritten: readonly O[],
): S;
type __VerterUseKeyOn<P, K extends PropertyKey> = P extends unknown
  ? K extends keyof P
    ? true
    : false
  : never;
type __VerterUseDirectOk<P, K extends PropertyKey> = 0 extends 1 & P
  ? true
  : string extends keyof P
    ? true
    : number extends keyof P
      ? true
      : symbol extends keyof P
        ? true
        : [__VerterUseKeyOn<P, K>] extends [false]
          ? false
          : true;
declare function __VerterUseDirect<C, I, K extends PropertyKey>(
  component: C,
  instance: I,
  key: 0 extends 1 & C
    ? K
    : __VerterUseDirectOk<I extends { readonly $props: infer P } ? P : never, K> extends true
      ? K
      : never,
): void;

export type Instance = InstanceType<typeof Child>;
export const stp20DefinitionTarget: typeof Child = Child;

// Rendered from `const { title = "fallback" } = defineProps<{ title: string; count: number; flag?: boolean }>()`.
// Callers omit the destructured default and the boolean-cast flag; setup reads both as defined.
type __VerterCallerOptionalKeys = "title" | "flag";
type __VerterSetupDefinedKeys = "title" | "flag";
type __VerterCallerRequiredKeys = "count";
type __VerterReactiveDefaultKeys = "title";
type __VerterBooleanCastKeys = "flag";
type __VerterBooleanEmptyStringKeys = never;
type __VerterValidatorKeys = never;
type DefaultedDeclared = { title: string; count: number; flag?: boolean };
type CallerOfDefaulted = Omit<DefaultedDeclared, __VerterCallerOptionalKeys> &
  Partial<Pick<DefaultedDeclared, Extract<__VerterCallerOptionalKeys, keyof DefaultedDeclared>>>;
export const callerMayOmitDefault: CallerOfDefaulted = { count: 1 };
type SetupTitle = DefaultedDeclared[Extract<__VerterSetupDefinedKeys, "title">];
export const setupDefault: SetupTitle = "fallback";
type _RequiredIsNotOptional = __VerterCallerRequiredKeys & __VerterCallerOptionalKeys extends never
  ? true
  : never;
const _requiredIsNotOptional: _RequiredIsNotOptional = true;
type _ReactiveDefaultIsOptional = __VerterReactiveDefaultKeys extends __VerterCallerOptionalKeys
  ? true
  : never;
const _reactiveDefaultIsOptional: _ReactiveDefaultIsOptional = true;
type _BooleanCastIsDefined = __VerterBooleanCastKeys extends __VerterSetupDefinedKeys
  ? true
  : never;
const _booleanCastIsDefined: _BooleanCastIsDefined = true;
type _NoValidator = __VerterValidatorKeys extends never ? true : never;
const _noValidator: _NoValidator = true;

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

const spread = { title: 1, kind: "number", value: 1 } as const;
// `title: 1` cannot satisfy `title: string`. It is still accepted because a
// later definite write replaces it, including when another v-bind follows
// that write. Other finite keys still participate in the check.
export const overwritten = new (__VerterUseConstructor(Child))({
  ...__VerterUseSpread(Child, spread, ["title"] as const),
  title: "new",
});

declare const open: Record<string, unknown>;
export const openDomain = __VerterUseSpread(Child, open, [] as const);

declare const anySpread: any;
export const anyDomain = __VerterUseSpread(Child, anySpread, [] as const);

declare const maybeOpen: { title: string; kind: "number"; value: number } | Record<string, unknown>;
export const unionOpen = __VerterUseSpread(Child, maybeOpen, [] as const);

type BranchProps = { kind: "number"; value: number } | { kind: "text"; label: string };
declare const Branch: { new (props: BranchProps): { readonly $props: BranchProps } };
const numberBranch = { kind: "number", value: 1 } as const;
export const branchNumber = __VerterUseSpread(Branch, numberBranch, [] as const);
const textBranch = { kind: "text", label: "ok" } as const;
export const branchText = __VerterUseSpread(Branch, textBranch, [] as const);

// Excess keys that arrive through a v-bind object, named or inline, are not
// rejected. Known-key types on that object still are.
const extraNamed = { title: "ok", kind: "number" as const, value: 1, titel: "extra" };
export const extraNamedSpread = __VerterUseSpread(Child, extraNamed, [] as const);
export const extraInlineSpread = __VerterUseSpread(
  Child,
  { title: "ok", kind: "number" as const, value: 1, titel: "inline" },
  [] as const,
);

// A generic spread is checked through its constraint, including extra keys
// the constraint adds. It is not collapsed to `never` and not widened to `any`.
export function constrainedSpread<T extends { title: string; kind: "number"; value: number }>(
  value: T,
) {
  return __VerterUseSpread(Child, value, [] as const);
}
export function constrainedExtraSpread<
  T extends { title: string; kind: "number"; value: number; extra: boolean },
>(value: T) {
  return __VerterUseSpread(Child, value, [] as const);
}

const mutableTuple = {
  title: "ok",
  kind: "number" as const,
  value: 1,
  tuple: ["a", 1] as [string, number],
};
export const mutableTupleSpread = __VerterUseSpread(Child, mutableTuple, [] as const);

declare const childInstance: ChildInstance;
export const directKnownKey = __VerterUseDirect(Child, childInstance, "title");
export const directUnionKey = __VerterUseDirect(Child, childInstance, "value");
