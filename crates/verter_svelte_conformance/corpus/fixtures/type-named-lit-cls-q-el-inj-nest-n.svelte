<svelte:options css="injected" />

<div class="a a&copy;b">x</div>

<style>
	p {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
