<script>
  let { pr } = $props();
  function ev() {}
</script>
{#await pr}<p>l</p>{:then v}<button onfocus={ev}>x</button>{/await}
