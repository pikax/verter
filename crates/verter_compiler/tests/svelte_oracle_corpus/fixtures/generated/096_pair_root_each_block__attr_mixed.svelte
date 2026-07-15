<script>
  let { acls, items } = $props();
</script>
{#each items as it}<p class="a {acls} b">hi</p>{/each}
