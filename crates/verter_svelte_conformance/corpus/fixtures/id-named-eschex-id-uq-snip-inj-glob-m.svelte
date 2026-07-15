<svelte:options css="injected" />

{#snippet subject()}
	<div id=a&amp;b>x</div>
{/snippet}

{@render subject()}

<style>
	:global(#a\26 b) {
		color: red;
	}
</style>
