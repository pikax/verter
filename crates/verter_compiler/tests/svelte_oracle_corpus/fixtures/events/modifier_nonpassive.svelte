<script>
	let count = $state(0);
</script>
<button on:click|nonpassive={() => count++}>go {count}</button>
