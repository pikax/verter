<svelte:options css="injected" />

{#snippet subject()}
	<div class=a&amp;b&amp;c>x</div>
{/snippet}

{@render subject()}

<style>
	:global(.a\26 b\&c) {
		color: red;
	}
</style>
