<script lang="ts">
  // Fixed workload fixture. Svelte runes carrier over the same shared
  // cross-file type surface as the Vue fixtures, so a shared-module edit
  // invalidates both carriers' reverse-dependency sets. Frozen bytes.
  import type { Labelled, PropsBase } from "../shared/props-base";
  import { defaultTheme } from "../shared/theme";

  let { rows, density, selected = null }: PropsBase<Labelled> = $props();

  let accent = $state(defaultTheme.accent);
  let count = $derived(rows.length);
</script>

<section class={density} style="color: {accent}">
  {#each rows as row (row.id)}
    <button onclick={() => (selected = row)}>{row.label}</button>
  {/each}
  <footer>{count}</footer>
</section>
