<script>
  let { cond } = $props();
  function ev() {}
</script>
{#if cond}<button onclick={ev}>x</button>{/if}
