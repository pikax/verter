<script>
	let n = $state(0);
</script>

<svelte:boundary onerror={() => n++}>
	<p>content</p>
</svelte:boundary>
