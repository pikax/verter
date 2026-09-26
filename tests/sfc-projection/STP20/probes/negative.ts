type ChildProps = { title: string; kind: "number"; value: number };
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

const misspelled = { titel: "typo" };
// A finite v-bind object cannot bypass the component's declared keys.
__VerterUseSpread(Child, misspelled, [] as const);

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
