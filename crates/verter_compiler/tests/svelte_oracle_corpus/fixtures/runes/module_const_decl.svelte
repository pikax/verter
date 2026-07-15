<script module>
	export const VERSION = '1';
	function helper() {
		return VERSION;
	}
</script>

<script>
	let count = $state(0);
</script>

<button onclick={() => count++}>{count}</button>
