<svelte:options css="injected" />

<slot>
	<div class="a a&b">x</div>
</slot>

<style>
	[data-x="\7a z"] {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
