<svelte:options customElement={{ tag: 'x-props', shadow: 'open', props: { count: { reflect: true, type: 'Number' } } }} />

<script>
	let { count = 7 } = $props();
</script>

<h1>{count}</h1>
