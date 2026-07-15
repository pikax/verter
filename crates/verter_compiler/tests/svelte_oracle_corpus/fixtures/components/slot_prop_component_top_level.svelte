<script>
	import Inner from './Inner.svelte';
	let { x } = $props();
</script>

<Inner slot="top" label={x} />
