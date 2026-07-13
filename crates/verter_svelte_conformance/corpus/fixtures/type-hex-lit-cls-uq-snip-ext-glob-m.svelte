{#snippet subject()}
	<div class=a&#x20;b>x</div>
{/snippet}

{@render subject()}

<style>
	:global(div) {
		color: red;
	}
</style>
