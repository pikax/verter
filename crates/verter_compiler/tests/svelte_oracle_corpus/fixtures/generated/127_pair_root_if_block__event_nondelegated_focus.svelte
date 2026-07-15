<script>
  let { cond } = $props();
  function ev() {}
</script>
{#if cond}<button onfocus={ev}>x</button>{/if}
