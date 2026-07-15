<script>
	let count = $state(0);
</script>
<button on:click|trusted={() => count++}>go {count}</button>
