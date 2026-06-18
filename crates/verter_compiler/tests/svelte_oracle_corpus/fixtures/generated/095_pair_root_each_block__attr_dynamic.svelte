<script>
  let { aid, items } = $props();
</script>
{#each items as it}<p id={aid}>hi</p>{/each}
