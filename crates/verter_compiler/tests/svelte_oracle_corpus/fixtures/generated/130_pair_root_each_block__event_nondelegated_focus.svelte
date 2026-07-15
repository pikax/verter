<script>
  let { items } = $props();
  function ev() {}
</script>
{#each items as it}<button onfocus={ev}>x</button>{/each}
