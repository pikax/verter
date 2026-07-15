<script>
	let count = $state(0);
</script>
<button on:click|stopImmediatePropagation={() => count++}>go {count}</button>
