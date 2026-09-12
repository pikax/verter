<script lang="ts">
  // Fixed workload fixture. Snippet + generic surface, so the Svelte arm of
  // the workload covers a second distinct resolution shape. Frozen bytes.
  import type { Identified, PropsBase, Selection } from "../shared/props-base";
  import type { ThemeSlice } from "../shared/theme";

  type Props<T extends Identified> = PropsBase<T> & ThemeSlice<"spacing">;

  let { rows, density, spacing, selected = null }: Props<Identified> = $props();

  function current(): Selection<Identified>["selected"] {
    return selected;
  }
</script>

{#snippet entry(row: Identified)}
  <li data-density={density}>{row.id}</li>
{/snippet}

<ul style="gap: {spacing[density]}px">
  {#each rows as row (row.id)}
    {@render entry(row)}
  {/each}
</ul>
{#if current()}
  <span>selected</span>
{/if}
