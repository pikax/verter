{#snippet subject()}
	<div class="a &#98;&#x62;">x</div>
{/snippet}

{@render subject()}

<style>
	p {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
