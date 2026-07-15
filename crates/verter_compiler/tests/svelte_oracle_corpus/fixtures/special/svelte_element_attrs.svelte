<script>
	let tag = $state('div');
	let cls = $state('a');
	let n = $state(0);
</script>

<svelte:element this={tag} class={cls} onclick={() => n++}>hi</svelte:element>
