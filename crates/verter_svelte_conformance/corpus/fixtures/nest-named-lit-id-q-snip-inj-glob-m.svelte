<svelte:options css="injected" />

{#snippet subject()}
	<div id="a&copy;b">x</div>
{/snippet}

{@render subject()}

<style>
	:global(&) {
		color: red;
	}
</style>
