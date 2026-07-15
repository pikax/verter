<script>
  let { kk, rest } = $props();
</script>
{#key kk}<p {...rest}>hi</p>{/key}
