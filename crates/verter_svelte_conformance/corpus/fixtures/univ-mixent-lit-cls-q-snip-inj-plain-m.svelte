<svelte:options css="injected" />

{#snippet subject()}
	<div class="a &#98;&#x62;">x</div>
{/snippet}

{@render subject()}

<style>
	* {
		color: red;
	}
</style>
