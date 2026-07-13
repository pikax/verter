<svelte:options css="injected" />

<slot>
	<div id="b">x</div>
</slot>

<style>
	:global(div) {
		color: red;
	}
</style>
