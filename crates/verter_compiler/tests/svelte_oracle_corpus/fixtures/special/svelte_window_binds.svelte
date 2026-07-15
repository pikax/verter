<script>
	let iw = $state(0);
	let ih = $state(0);
	let sx = $state(0);
	let on = $state(false);
	let f = $state(false);
	let dpr = $state(1);
</script>

<svelte:window
	bind:innerWidth={iw}
	bind:innerHeight={ih}
	bind:scrollX={sx}
	bind:online={on}
	bind:focused={f}
	bind:devicePixelRatio={dpr}
/>
