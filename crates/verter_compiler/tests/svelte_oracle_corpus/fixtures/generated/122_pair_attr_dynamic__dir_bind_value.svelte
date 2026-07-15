<script>
  let { aid } = $props();
  let dv = $state('');
</script>
<input id={aid} bind:value={dv} />
