<script>
	let count = $state(0);
</script>
<button on:click|self={() => count++}>go {count}</button>
