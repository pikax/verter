<script>
	import Child from './Child.svelte';
	let { p } = $props();
	let v = $state(0);
</script>

<Child bind:value={() => v, (nv) => v = nv} />
