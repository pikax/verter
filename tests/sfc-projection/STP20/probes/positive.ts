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
type __VerterUseBranch<S, P> =
  Exclude<keyof S, keyof P> extends never ? ([S] extends [Partial<P>] ? true : false) : false;
type __VerterUseChecked<S, P, O extends PropertyKey> = S extends unknown
  ? [true] extends [P extends unknown ? __VerterUseBranch<Omit<S, O>, Omit<P, O>> : never]
    ? true
    : false
  : never;
type __VerterUseKnownSpread<S, P, O extends PropertyKey> = [__VerterUseResolvedKeys<S>] extends [
  never,
]
  ? S
  : __VerterUseResolvedKeys<S> extends 1
    ? [__VerterUseMemberOpen<S>] extends [false]
      ? [__VerterUseChecked<S, P, O>] extends [true]
        ? S
        : never
      : S
    : S;
declare function __VerterUseSpread<C, S, O extends PropertyKey>(
  component: C,
  spread: S & __VerterUseKnownSpread<S, __VerterUseComponentProps<C>, O>,
  overwritten: readonly O[],
): S;

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
