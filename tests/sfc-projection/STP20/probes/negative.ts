type ChildProps = { title: string; kind: "number"; value: number };
interface ChildInstance {
  readonly $props: ChildProps;
}
declare const Child: { new (props: ChildProps): ChildInstance };

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

const misspelled = { titel: "typo" };
// A finite v-bind object cannot bypass the component's declared keys.
__VerterUseSpread(Child, misspelled, [] as const);

const staleTitle = { title: 1, kind: "number", value: 1 } as const;
// The same number is rejected when the key is not certainly overwritten.
__VerterUseSpread(Child, staleTitle, [] as const);
