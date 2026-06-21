<script>
	let count = $state(0);
	function bump() { count++; }
</script>
<button onclick={bump}>{count}</button>
