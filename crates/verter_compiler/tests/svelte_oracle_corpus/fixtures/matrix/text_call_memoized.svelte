<script>
	let count = $state(0);
	function double(x) { return x * 2; }
</script>
<button onclick={() => count++}>{double(count)}</button>
