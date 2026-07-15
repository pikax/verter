<script>
	import Child from './Child.svelte';
	let { p } = $props();
</script>

<Child on:foo={() => console.log('foo')} />
