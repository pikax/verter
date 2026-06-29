<script>
	let count = $state(0);
</script>
<button on:click|preventDefault|stopPropagation={() => count++}>go {count}</button>
