<script>
	let count = $state(0);
</script>
<button on:click|stopPropagation={() => count++}>go {count}</button>
