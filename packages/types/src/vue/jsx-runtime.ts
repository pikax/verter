/**
 * Seal Vue's automatic JSX namespace against foreign ambient children rules.
 *
 * Vue's official `vue/jsx-runtime` intentionally omits
 * `ElementChildrenAttribute`: Vue templates model nested content as slots, not
 * as a React-style `children` prop. TypeScript otherwise falls back to a global
 * JSX namespace for the missing member, so an installed `@types/react` makes
 * valid Vue elements and components reject their nested content. Declaring the
 * member locally—and leaving it empty—preserves Vue's official behavior while
 * preventing that cross-framework fallback.
 */
declare module "vue/jsx-runtime" {
  namespace JSX {
    interface ElementChildrenAttribute {}
  }
}

export {};
