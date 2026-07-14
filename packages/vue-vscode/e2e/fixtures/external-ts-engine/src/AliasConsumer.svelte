<script lang="ts">
  // Same cross-section for the Svelte carrier: path-aliased import + ambient
  // global. Both fail under the inferred path; both resolve under the
  // configured project.
  import { makeShape, type AliasedShape } from "@/lib/helper";

  const shape: AliasedShape = makeShape(2, "world");
  const token = AMBIENT_TOKEN;
</script>

<div>{shape.label} {token}</div>
