<script>
	let count = $state(0);
</script>

<svelte:head>
	<title>page {count}</title>
</svelte:head>

<button onclick={() => count++}>inc</button>
