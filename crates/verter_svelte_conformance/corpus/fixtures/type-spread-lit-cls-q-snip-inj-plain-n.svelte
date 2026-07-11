<svelte:options css="injected" />

<script>
	let { rest } = $props();
</script>

{#snippet subject()}
	<div {...rest}>x</div>
{/snippet}

{@render subject()}

<style>
	p {
		color: red;
	}
</style>
