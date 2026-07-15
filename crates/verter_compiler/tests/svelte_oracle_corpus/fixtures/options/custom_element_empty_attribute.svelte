<svelte:options customElement={{ tag: 'x-empty-attr', props: { a: { attribute: "" } } }} />

<script>
	let { a } = $props();
</script>

<p>{a}</p>
