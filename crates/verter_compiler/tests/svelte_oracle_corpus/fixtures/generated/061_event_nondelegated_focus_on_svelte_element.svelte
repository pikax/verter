<script>
  let { tag } = $props();
  function ev() {}
</script>
<svelte:element this={tag} onfocus={ev}>x</svelte:element>
