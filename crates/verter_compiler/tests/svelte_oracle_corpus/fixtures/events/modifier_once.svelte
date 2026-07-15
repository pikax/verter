<script>
	let count = $state(0);
</script>
<button on:click|once={() => count++}>go {count}</button>
