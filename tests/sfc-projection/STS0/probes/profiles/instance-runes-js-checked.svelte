<script>
  // @ts-check
  let count = $state(0);
  /** @type {number} */
  const doubled = $derived(count * 2);
</script>

<p>{doubled}</p>
