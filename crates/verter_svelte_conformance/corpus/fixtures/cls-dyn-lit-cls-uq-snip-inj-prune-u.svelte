<svelte:options css="injected" />

<script>
	let { value } = $props();
</script>

{#snippet subject()}
	<div class={value}>x</div>
{/snippet}

{@render subject()}

<style>
	.b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
