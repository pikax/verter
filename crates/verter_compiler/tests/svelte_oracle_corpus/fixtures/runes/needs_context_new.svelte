<script>
	let now = $state(new Date());
	function refresh() {
		now = new Date();
	}
</script>
<button onclick={refresh}>{now.getFullYear()}</button>
