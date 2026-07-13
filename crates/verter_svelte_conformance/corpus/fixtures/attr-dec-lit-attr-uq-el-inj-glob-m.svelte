<svelte:options css="injected" />

<div data-x=&#98;>x</div>

<style>
	:global([data-x="b"]) {
		color: red;
	}
</style>
