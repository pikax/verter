<script>
	let count = $state(0);
</script>

<svelte:head>
	<title>n {String(count)} {count}</title>
</svelte:head>

<button onclick={() => count++}>inc</button>
