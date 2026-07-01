<script>
	let k = $state(0);
	let obj = { failed: null };
</script>

<input bind:value={obj.failed} />
<svelte:boundary failed={obj.failed}><p>hi</p></svelte:boundary>
