{#snippet subject()}
	<div data-x="&#98;&#x62;">x</div>
{/snippet}

{@render subject()}

<style>
	:global([data-x="bb"]) {
		color: red;
	}
</style>
