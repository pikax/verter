{#snippet subject()}
	<div class="wrap">
	<div id="a&#38;b&#38;c">x</div>
</div>
{/snippet}

{@render subject()}

<style>
	.wrap [data-x="\7a \z"] {
		color: red;
	}
</style>
