<script>
	let count = $state(0);
	$effect(() => { console.log(count); });
</script>
<button onclick={() => count++}>{count}</button>
