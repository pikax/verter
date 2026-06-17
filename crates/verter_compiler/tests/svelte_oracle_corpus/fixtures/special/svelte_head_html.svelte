<script>
	let title = $state('page');
	let markup = $state('<b>bold</b>');
</script>

<svelte:head>
	<title>{title}</title>
</svelte:head>

<div>{@html markup}</div>
