import "../index";

declare global {
  namespace JSX {
    // Hermetic React-shaped competitor. Without the module-local empty
    // declaration, TypeScript falls back to this member and checks nested Vue
    // slot content as a `children` prop.
    interface ElementChildrenAttribute {
      children: {};
    }
  }
}

declare const Child: new () => { $props: { label: string } };

// Nested Vue content is slot content. A competing ambient React
// `ElementChildrenAttribute` must not turn it into a checked `children` prop.
const intrinsic = <div class="card">content</div>;
const component = <Child label="ok">slot content</Child>;

// @ts-expect-error React's attribute spelling must not leak into Vue JSX.
const reactOnly = <div className="card" />;

void intrinsic;
void component;
void reactOnly;
