<svelte:options css="injected" />

{#snippet subject()}
	<div class="wrap">
	<div class="a a&copy;b">x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap * {
		color: red;
	}
</style>
