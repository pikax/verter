<script>
  let { items } = $props();
  function ev() {}
</script>
{#each items as it}<button onclick={ev}>x</button>{/each}
