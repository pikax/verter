<script>
	let box = $state({ a: 1, b: 2 });

	function bump() {
		box.a += 1;
	}
</script>

<button onclick={bump}>{box.a} {box.b}</button>
