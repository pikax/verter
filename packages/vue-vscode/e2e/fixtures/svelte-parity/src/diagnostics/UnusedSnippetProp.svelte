<script lang="ts">
  import type { Snippet } from "svelte";

  let { header, body }: { header?: Snippet; body?: Snippet } = $props();
</script>

<div class="snippet-contract">
  {@render body?.()}
</div>
