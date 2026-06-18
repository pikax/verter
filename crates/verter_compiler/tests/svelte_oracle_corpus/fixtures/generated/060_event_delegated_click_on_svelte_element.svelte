<script>
  let { tag } = $props();
  function ev() {}
</script>
<svelte:element this={tag} onclick={ev}>x</svelte:element>
