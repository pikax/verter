<script lang="ts">
  // Fixed workload fixture for the FAILED-CONSTRUCTION action class on the
  // Svelte arm. The rune destructuring is deliberately unterminated, so
  // construction fails, reservations are released, and nothing warms.
  // Frozen bytes — do not repair this file.
  import type { PropsBase, Labelled } from "../shared/props-base";

  let { rows, density : PropsBase<Labelled> = $props();
</script>

<ul>
  {#each rows as row}
