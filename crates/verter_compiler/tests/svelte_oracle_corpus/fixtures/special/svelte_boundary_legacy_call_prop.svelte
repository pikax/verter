<script>
	export let obj;
	export let fail;
</script>

<svelte:boundary failed={obj.m()} pending={fail}>
	<p>content</p>
</svelte:boundary>
