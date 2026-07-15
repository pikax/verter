<script>
	let count = $state(0);
</script>
<button on:click|capture={() => count++}>go {count}</button>
