<svelte:options css="injected" />

<slot>
	<div class="a&#x20;b">x</div>
</slot>

<style>
	.zz {
		& {
			color: red;
		}
	}
</style>
