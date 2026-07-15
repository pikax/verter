<script>
  let { items } = $props();
  function ev() {}
</script>
{#each items as it}<button onclickcapture={ev}>x</button>{/each}
