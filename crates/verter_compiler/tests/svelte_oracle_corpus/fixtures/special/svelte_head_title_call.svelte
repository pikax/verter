<script>
	let count = $state(0);
</script>

<svelte:head>
	<title>{String(count)}</title>
</svelte:head>

<button onclick={() => count++}>inc</button>
