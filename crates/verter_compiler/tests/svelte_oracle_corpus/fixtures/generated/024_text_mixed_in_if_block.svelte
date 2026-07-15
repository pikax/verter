<script>
  let { cond, tx } = $props();
</script>
{#if cond}<p>a {tx} b</p>{/if}
