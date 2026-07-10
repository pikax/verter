<script>
	let c = $state('card');
</script>

<button onclick={() => c += '!'} class={c}>x</button>

<style>
	button {
		color: red;
	}
</style>
