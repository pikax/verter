<script>
	import Child from './Child.svelte';
	let { x } = $props();
</script>

<Child on:foo-bar={() => x} />
