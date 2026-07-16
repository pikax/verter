import ExposePublic from "./ExposePublic.svelte";

type PublicExports = ReturnType<typeof ExposePublic>;

/**
 * Test-importer (*.spec.ts) for Svelte.
 *
 * Unlike Vue, Svelte currently ships **no** testing-API virtual file
 * (`*.svelte.__verter_test.ts`). Enabling `exposeBindingsTesting` must not
 * invent a second instance shape: `secretInternal` stays off the component type.
 */
export function readSecretMustStayPrivate(c: PublicExports): unknown {
  // @ts-expect-error - Svelte test importers use the same public callable component.
  return c.secretInternal;
}

export function readPublicCount(c: PublicExports): number {
  return c.publicCount;
}
