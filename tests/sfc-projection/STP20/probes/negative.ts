type ChildProps = {
  title: string;
  kind: "number";
  value: number;
  optional?: string;
  tuple?: [string, number];
};
interface ChildInstance {
  readonly $props: ChildProps;
}
declare const Child: { new (props: ChildProps): ChildInstance };

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
type __VerterUseTolerant<P, I> = (0 extends 1 & P
  ? I extends { readonly $props: infer Q }
    ? Q
    : 0 extends 1 & I
      ? P
      : {}
  : P) &
  Record<string, unknown>;
declare function __VerterUseConstructor<P, I>(
  component: abstract new (props: P) => I,
): new (props: __VerterUseTolerant<P, I>) => I;

declare const childInstance: ChildInstance;
// A misspelled key written directly as an attribute is rejected.
__VerterUseDirect(Child, childInstance, "titel");

const missingRequired = { titel: "typo" };
// A spread still has to provide required props.
__VerterUseSpread(Child, missingRequired, [] as const);

// An empty props argument does not satisfy a required prop.
new (__VerterUseConstructor(Child))({});

const staleTitle = { title: 1, kind: "number", value: 1 } as const;
// The same number is rejected when the key is not certainly overwritten.
__VerterUseSpread(Child, staleTitle, [] as const);

type BranchProps = { kind: "number"; value: number } | { kind: "text"; label: string };
declare const Branch: { new (props: BranchProps): { readonly $props: BranchProps } };
const crossBranch = { kind: "number", label: "no" } as const;
// A key that exists only on the other union arm is not a match for this arm.
__VerterUseSpread(Branch, crossBranch, [] as const);
const mismatched = { title: "ok", kind: "number", value: "x" } as const;
// A discriminated pair with the wrong value is not assignable to either arm.
__VerterUseSpread(Child, mismatched, [] as const);

const explicitUndefined = {
  title: "ok",
  kind: "number" as const,
  value: 1,
  optional: undefined,
};
// exactOptionalPropertyTypes rejects an explicit undefined on an optional prop.
__VerterUseSpread(Child, explicitUndefined, [] as const);

const readonlyTuple = {
  title: "ok",
  kind: "number" as const,
  value: 1,
  tuple: ["a", 1] as const,
};
// A readonly tuple is not assignable to a mutable tuple prop.
__VerterUseSpread(Child, readonlyTuple, [] as const);

// A naked type parameter and a constraint that is not assignable are checked
// through the constraint. Neither collapses the parameter type to `never`.
export function nakedSpread<T>(value: T) {
  return __VerterUseSpread(Child, value, [] as const);
}
export function wrongConstraintSpread<T extends { title: number; kind: "number"; value: number }>(
  value: T,
) {
  return __VerterUseSpread(Child, value, [] as const);
}
export function recordConstraintSpread<T extends Record<string, unknown>>(value: T) {
  return __VerterUseSpread(Child, value, [] as const);
}
