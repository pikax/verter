<svelte:options css="injected" />

<div data-x=a&amp;b>x</div>

<style>
	[data-x="\zz"] {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
