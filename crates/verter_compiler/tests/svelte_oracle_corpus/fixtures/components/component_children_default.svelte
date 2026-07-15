<script>
	import Child from './Child.svelte';
	let { x } = $props();
</script>

<Child>hello {x}</Child>
