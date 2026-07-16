import ExposePublic from "./ExposePublic.vue";

// Non-test .ts importer: should see public surface only.
export type PublicProps = InstanceType<typeof ExposePublic>;

export function readPublic(c: InstanceType<typeof ExposePublic>): number {
  // publicCount may be on instance; secretInternal must NOT type-check as present.
  return (c as { publicCount?: number }).publicCount ?? 0;
}

export function readSecretFromPublicSurface(c: InstanceType<typeof ExposePublic>): unknown {
  // @ts-expect-error - non-test importers must not see setup-only bindings.
  return c.secretInternal;
}
