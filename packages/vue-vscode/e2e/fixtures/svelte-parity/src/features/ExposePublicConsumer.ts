import ExposePublic from "./ExposePublic.svelte";
import type { ComponentProps } from "svelte";

// Non-test importer: public surface only.
export type PublicProps = ComponentProps<typeof ExposePublic>;
export type PublicExports = ReturnType<typeof ExposePublic>;

export function readPublic(c: PublicExports): number {
  return c.publicCount;
}

export function readSecretFromPublicSurface(c: PublicExports): unknown {
  // @ts-expect-error - Svelte's callable public exports must not expose private state.
  return c.secretInternal;
}
