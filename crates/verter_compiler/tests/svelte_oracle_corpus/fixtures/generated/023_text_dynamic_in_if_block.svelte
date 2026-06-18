<script>
  let { cond, tx } = $props();
</script>
{#if cond}<p>{tx}</p>{/if}
