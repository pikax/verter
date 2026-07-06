<script>
	class S {
		subscribe(fn) {
			fn(1);
			return () => {};
		}
	}

	const c = new S();
</script>

<p>{$c}</p>
