<script>
	let count = $state(0);
</script>
<button on:click|capture|preventDefault={() => count++}>go {count}</button>
