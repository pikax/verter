<script>
	import Child from './Child.svelte';
	let { name } = $props();
</script>

<Child label="hi" {name} count={1 + 2} />
