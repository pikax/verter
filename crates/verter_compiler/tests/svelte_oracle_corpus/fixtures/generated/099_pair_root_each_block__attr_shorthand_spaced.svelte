<script>
  let { items, value } = $props();
</script>
{#each items as it}<p { value }>hi</p>{/each}
