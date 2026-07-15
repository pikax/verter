<script>
	let tag = $state('div');
	let count = $state(0);
</script>

<svelte:element this={tag} on:click|preventDefault={() => count++} on:keydown|capture={() => count++}>hi</svelte:element>
