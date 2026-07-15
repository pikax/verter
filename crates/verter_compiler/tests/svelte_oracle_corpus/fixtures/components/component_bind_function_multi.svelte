<script>
	import Child from './Child.svelte';
	let v = $state(0);
	let w = $state(1);
</script>

<Child bind:value={() => v, (nv) => v = nv} bind:other={() => w, (nw) => w = nw} />
