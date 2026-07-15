<script>
	let count = $state(0);
	/*! general-license */
	const STEP = 2;

	function advance(delta = STEP) {
		count += delta;
	}

	class Marker {
		value = 3;
	}

	console.log(STEP);

	for (let index = 0; index < 1; index += 1) {
		if (index < 0) count += index;
	}

	if (new Marker().value < 0) {
		advance();
	}
</script>

<button onclick={advance}>{count}</button>
