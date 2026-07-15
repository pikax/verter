<svelte:options css="injected" />

{#snippet subject()}
	<div data-x>x</div>
{/snippet}

{@render subject()}

<style>
	p {
		color: red;
	}
</style>
