<svelte:options css="injected" />

<div data-x=a&#x26;b>x</div>

<style>
	[data-x="\zz"] {
		color: red;
	}
</style>
