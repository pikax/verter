<script>
	let { xs } = $props();
</script>

<svelte:boundary failed={[...xs]}><p>hi</p></svelte:boundary>
