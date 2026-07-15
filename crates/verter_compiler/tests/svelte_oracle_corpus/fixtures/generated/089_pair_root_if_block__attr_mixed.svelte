<script>
  let { acls, cond } = $props();
</script>
{#if cond}<p class="a {acls} b">hi</p>{/if}
