<script>
	let count = $state(0);
	function compute() { return count * 2; }
	let doubled = $derived.by(compute);
</script>
<button onclick={() => count++}>{doubled}</button>
