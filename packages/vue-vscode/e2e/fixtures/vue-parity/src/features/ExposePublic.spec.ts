import ExposePublic from "./ExposePublic.vue";

/**
 * Test-importer (*.spec.ts): with `verter.experimental.exposeBindingsTesting`
 * the VTU-style testing surface must expose script-setup bindings that
 * `defineExpose` did not publish.
 */
export function readSecretFromTestingSurface(
  c: InstanceType<typeof ExposePublic>,
): string | { value: string } {
  return c.secretInternal;
}

export function readPublicFromTestingSurface(
  c: InstanceType<typeof ExposePublic>,
): number | { value: number } {
  return c.publicCount;
}
