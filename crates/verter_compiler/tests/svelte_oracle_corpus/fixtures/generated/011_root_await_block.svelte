<script>
  let { pr } = $props();
</script>
{#await pr}<p>load</p>{:then v}<p>hello</p>{/await}
