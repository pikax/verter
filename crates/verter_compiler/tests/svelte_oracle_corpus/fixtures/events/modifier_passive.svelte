<script>
	let count = $state(0);
</script>
<button on:click|passive={() => count++}>go {count}</button>
