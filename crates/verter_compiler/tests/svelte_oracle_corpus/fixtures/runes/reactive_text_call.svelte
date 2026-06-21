<script module>
	function double(x) {
		return x * 2;
	}
</script>
<script>
	let count = $state(0);
</script>
<button onclick={() => count++}>{double(count)}</button>
