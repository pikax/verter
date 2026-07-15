<script>
	let count = $state(0);
</script>
<button on:click|preventDefault={() => count++}>go {count}</button>
