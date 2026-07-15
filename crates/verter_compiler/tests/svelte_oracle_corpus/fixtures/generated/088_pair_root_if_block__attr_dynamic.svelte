<script>
  let { aid, cond } = $props();
</script>
{#if cond}<p id={aid}>hi</p>{/if}
