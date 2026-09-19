<script>
  // @ts-nocheck
  let count = $state(0);
  const doubled = $derived(count * 2);
</script>

<p>{doubled}</p>
