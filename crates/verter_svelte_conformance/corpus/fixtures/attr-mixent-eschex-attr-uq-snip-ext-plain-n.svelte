{#snippet subject()}
	<div data-x=a&#38;&#x62;>x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="\7a z"] {
		color: red;
	}
</style>
