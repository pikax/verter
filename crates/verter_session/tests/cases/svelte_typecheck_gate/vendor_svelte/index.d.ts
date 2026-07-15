// Vendored minimal `svelte` type declarations for the type-check validity
// gate (Testing-Hermeticity: no npm install, no third-party checkout). Pinned
// to the audited Svelte 5.56.x surface the projection depends on. This is a
// DELIBERATELY minimal subset — enough to type-check the projected `.svelte.tsx`
// and discriminate the rune/snippet/event fixtures, NOT the full package.

// The branded Snippet type — a callable that returns the snippet result.
declare const SnippetBrand: unique symbol;
export interface Snippet<Params extends unknown[] = []> {
  (
    this: void,
    ...args: Params
  ): {
    "{@render ...} must be called with a Snippet": "import type { Snippet } from 'svelte'";
  } & { readonly [SnippetBrand]: true };
}

// The async-family + lifecycle runtime imports are transparent: declare
// the ones the gate fixtures might import so they resolve.
export function hydratable<T>(key: string, fn: () => T): T;
export function getAbortSignal(): AbortSignal;
export function createContext<T>(): { get(): T; set(value: T): void };
