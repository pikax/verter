// JSX is the one syntax TypeScript resolves against construct signatures,
// else call signatures, of the same value, with every overload and binder.
// It is recorded here as the only uniform form; a `.tsx` checking module
// cannot host `lang="ts"` setup code verbatim (`<T>value` assertions), and
// a JSX element's type is `JSX.Element`, so it yields no specialized
// instance for the use's observations.
export {};

declare global {
  namespace JSX {
    interface Element {
      readonly __element: true;
    }
    interface IntrinsicElements {}
    interface ElementAttributesProperty {
      $props: {};
    }
  }
}

declare function Mix(props: { kind: "a"; n: number }): JSX.Element;
declare function Mix<T extends string>(props: {
  kind: "g";
  value: T;
  pick?: (value: T) => void;
}): JSX.Element;
declare function Mix(props: { kind: "z"; s: string }): JSX.Element;
declare function Pair<T extends string>(props: { kind: "list"; items: T[] }): JSX.Element;
declare function Pair<T extends string>(props: {
  kind: "one";
  item: T;
  echo?: (item: T) => void;
}): JSX.Element;
declare const Picker: new <const T extends { id: number }>(props: {
  items: readonly T[];
  pick?: (item: T) => void;
}) => { readonly $props: { items: readonly T[]; pick?: (item: T) => void } };

export const uses = [
  <Mix kind="a" n={1} />,
  <Mix
    kind="g"
    value="v"
    pick={(value) => {
      const literal: "v" = value;
      return literal;
    }}
  />,
  <Mix kind="z" s="s" />,
  <Pair
    kind="one"
    item="x"
    echo={(item) => {
      const literal: "x" = item;
      return literal;
    }}
  />,
  <Pair kind="list" items={["x"]} />,
  <Picker
    items={[{ id: 1 }]}
    pick={(item) => {
      const one: 1 = item.id;
      return one;
    }}
  />,
  // @ts-expect-error TS2769 (TS 6 reports it at the tag, TS 7 at `kind`)
  <Mix kind="a" n="1" />,
];
