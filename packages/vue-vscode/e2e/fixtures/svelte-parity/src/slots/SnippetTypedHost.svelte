<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Typed snippet (slot) host — Svelte first-class equivalent of defineSlots.
   */
  let {
    header,
    row,
    footer,
  }: {
    header: Snippet<[{ title: string; count: number }]>;
    row: Snippet<[{ body: string; flag: boolean }]>;
    footer?: Snippet<[{ ok: boolean }]>;
  } = $props();
</script>

<header>
  {@render header({ title: "hdr", count: 3 })}
</header>
<main>
  {@render row({ body: "main-body", flag: true })}
</main>
{#if footer}
  <footer>
    {@render footer({ ok: true })}
  </footer>
{/if}
