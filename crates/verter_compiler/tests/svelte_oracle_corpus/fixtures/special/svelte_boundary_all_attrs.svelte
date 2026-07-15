<script>
	let n = $state(0);
	let { failed, pending } = $props();
</script>

<svelte:boundary onerror={() => n++} failed={failed} pending={pending}>
	<p>content</p>
</svelte:boundary>
