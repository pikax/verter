<svelte:options css="injected" />

<slot>
	<div class="a a&#38;b&#38;c">x</div>
</slot>

<style>
	.a\26 b\&c {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
