<svelte:options css="injected" />

{#snippet subject()}
	<div data-x=&#98;>x</div>
{/snippet}

{@render subject()}

<style>
	:global(div) {
		color: red;
	}
</style>
