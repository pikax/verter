<script>
	import Inner from './Inner.svelte';
	let { x } = $props();
</script>

<div><Inner slot="bar" label={x} /></div>
