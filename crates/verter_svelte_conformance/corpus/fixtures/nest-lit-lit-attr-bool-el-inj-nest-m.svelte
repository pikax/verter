<svelte:options css="injected" />

<div data-x>x</div>

<style>
	[data-x] {
		& {
			color: red;
		}
	}
</style>
