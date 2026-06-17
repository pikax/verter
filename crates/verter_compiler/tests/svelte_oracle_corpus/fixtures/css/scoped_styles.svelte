<script>
	let active = $state(false);
</script>

<div class="card" class:active>
	<h2>title</h2>
	<p>body</p>
</div>

<style>
	.card {
		color: blue;
		padding: 1rem;
	}

	.card.active {
		color: green;
	}

	h2 {
		font-weight: bold;
	}
</style>
