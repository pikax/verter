<svelte:options css="injected" />

{#snippet subject()}
	<div class="wrap">
	<div class="a a&#x26;b">x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap .a\26 b {
		color: red;
	}
</style>
