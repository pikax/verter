<script>
  let { items, rest } = $props();
</script>
{#each items as it}<p {...rest}>hi</p>{/each}
