<script>
  let { cond, rest } = $props();
</script>
{#if cond}<p {...rest}>hi</p>{/if}
