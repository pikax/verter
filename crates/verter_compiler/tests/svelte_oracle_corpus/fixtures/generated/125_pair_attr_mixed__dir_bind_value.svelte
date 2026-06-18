<script>
  let { acls } = $props();
  let dv = $state('');
</script>
<input class="a {acls} b" bind:value={dv} />
