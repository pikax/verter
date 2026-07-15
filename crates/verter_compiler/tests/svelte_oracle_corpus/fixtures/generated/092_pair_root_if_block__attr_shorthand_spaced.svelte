<script>
  let { cond, value } = $props();
</script>
{#if cond}<p { value }>hi</p>{/if}
