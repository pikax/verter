{#snippet subject()}
	<div class="wrap">
	<div id=a&#x26;b>x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap [data-x="\7a z"] {
		color: red;
	}
</style>
