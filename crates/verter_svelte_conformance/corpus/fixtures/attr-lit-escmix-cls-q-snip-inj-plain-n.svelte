<svelte:options css="injected" />

{#snippet subject()}
	<div class="a a&b&c">x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="\7a \z"] {
		color: red;
	}
</style>
