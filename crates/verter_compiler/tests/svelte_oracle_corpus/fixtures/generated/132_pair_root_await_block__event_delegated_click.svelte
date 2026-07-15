<script>
  let { pr } = $props();
  function ev() {}
</script>
{#await pr}<p>l</p>{:then v}<button onclick={ev}>x</button>{/await}
