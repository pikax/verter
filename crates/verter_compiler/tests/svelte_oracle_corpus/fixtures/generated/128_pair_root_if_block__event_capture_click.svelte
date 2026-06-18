<script>
  let { cond } = $props();
  function ev() {}
</script>
{#if cond}<button onclickcapture={ev}>x</button>{/if}
