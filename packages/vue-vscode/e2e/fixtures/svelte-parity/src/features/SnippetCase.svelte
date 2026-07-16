<script lang="ts">
  import type { Snippet } from "svelte";

  let { row }: { row: Snippet<[{ name: string }]> } = $props();
</script>

{@render row({ name: "Ada" })}
