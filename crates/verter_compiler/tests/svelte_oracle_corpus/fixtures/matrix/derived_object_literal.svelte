<script>
	let count = $state(0);
	let box = $derived({ value: count });
</script>
<button onclick={() => count++}>{box.value}</button>
