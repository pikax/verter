<script>
	let n = $state(0);
	let ae = $state(0);
	let fe = $state(0);
	let vs = $state(0);
</script>

<svelte:document
	onvisibilitychange={() => n++}
	bind:activeElement={ae}
	bind:fullscreenElement={fe}
	bind:visibilityState={vs}
/>
