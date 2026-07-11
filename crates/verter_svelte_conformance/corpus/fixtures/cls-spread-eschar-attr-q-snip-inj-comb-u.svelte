<svelte:options css="injected" />

<script>
	let { rest } = $props();
</script>

{#snippet subject()}
	<div class="wrap">
	<div {...rest}>x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap .a\&b {
		color: red;
	}
</style>
