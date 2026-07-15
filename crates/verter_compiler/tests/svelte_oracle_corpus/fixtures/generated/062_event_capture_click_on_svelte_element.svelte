<script>
  let { tag } = $props();
  function ev() {}
</script>
<svelte:element this={tag} onclickcapture={ev}>x</svelte:element>
