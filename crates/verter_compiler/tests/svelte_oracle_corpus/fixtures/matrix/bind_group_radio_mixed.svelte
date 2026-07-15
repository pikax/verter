<script>
	let selected = $state("a");
	let label = $state("a");
</script>
<input type="radio" bind:group={selected} value="pre-{label}" />
<button onclick={() => label = "b"}>x</button>
