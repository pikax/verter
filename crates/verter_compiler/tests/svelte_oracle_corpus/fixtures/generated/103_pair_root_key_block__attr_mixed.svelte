<script>
  let { acls, kk } = $props();
</script>
{#key kk}<p class="a {acls} b">hi</p>{/key}
