<script>
	let count = $state(0);
</script>
<button onclick={() => count++}>{count > 0 ? count : 0}</button>
