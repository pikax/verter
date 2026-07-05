<svelte:options customElement={{ tag: 'x-id' }} />

<script>
	let { 'data-id': dataId } = $props();
</script>

<p>{dataId}</p>
