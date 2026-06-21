<script>
	let count = $state(0);
	function log(x) { return x; }
</script>
<button onclick={() => log(++count)}>{count}</button>
