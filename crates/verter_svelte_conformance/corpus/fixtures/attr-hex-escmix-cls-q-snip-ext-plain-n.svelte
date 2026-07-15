{#snippet subject()}
	<div class="a a&#x26;b&#x26;c">x</div>
{/snippet}

{@render subject()}

<style>
	[data-x="\7a \z"] {
		color: red;
	}
</style>
