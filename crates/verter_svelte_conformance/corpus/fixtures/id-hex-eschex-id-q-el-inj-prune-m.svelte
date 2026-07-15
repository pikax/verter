<svelte:options css="injected" />

<div id="a&#x26;b">x</div>

<style>
	#a\26 b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
