// Minimal JSX runtime so `jsx: react-jsx` + `jsxImportSource: verterjsx` resolves.
// Proves the carrier's JSX is type-checked under the real tsconfig's jsx settings.
export namespace JSX {
  interface IntrinsicElements {
    div: { id?: string; class?: string; children?: unknown };
    span: { title?: string; children?: unknown };
  }
  interface Element {
    __brand: "verter-jsx-element";
  }
  interface ElementChildrenAttribute {
    children: {};
  }
}

export function jsx(type: any, props: any, key?: any): JSX.Element;
export function jsxs(type: any, props: any, key?: any): JSX.Element;
export const Fragment: unique symbol;
