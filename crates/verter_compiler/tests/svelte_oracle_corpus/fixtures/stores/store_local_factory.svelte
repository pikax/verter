<script>
	function w(v) {
		const subs = [];

		return {
			subscribe(f) {
				subs.push(f);
				f(v);
				return () => {};
			},
			set(x) {
				subs.forEach((fn) => fn(x));
			}
		};
	}

	const c = w(0);
</script>

<p>{$c}</p>
