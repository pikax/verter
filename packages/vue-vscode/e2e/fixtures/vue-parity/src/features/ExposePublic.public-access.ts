import ExposePublic from "./ExposePublic.vue";

/**
 * Non-test importer: must remain on the public surface even when
 * `exposeBindingsTesting` is enabled for the workspace.
 * Direct access to `secretInternal` should be a type error.
 */
export function readSecretFromPublicSurface(c: InstanceType<typeof ExposePublic>): unknown {
  // @ts-expect-error public surface must not include secretInternal
  return c.secretInternal;
}

export function readPublicFromPublicSurface(
  c: InstanceType<typeof ExposePublic>,
): number | { value: number } | undefined {
  return (c as { publicCount?: number | { value: number } }).publicCount;
}
