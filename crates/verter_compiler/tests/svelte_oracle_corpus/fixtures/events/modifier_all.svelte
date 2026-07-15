<script>
	let count = $state(0);
</script>
<button on:click|once|trusted|self|preventDefault|stopImmediatePropagation|stopPropagation={() => count++}>go {count}</button>
