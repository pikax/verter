<script>
    let title = $state('a');
    let html = $state('<strong>one</strong>');
</script>
<div {...{ title }}>{@html html}</div>
<button onclick={() => title += '!'}>title</button>
<button onclick={() => html = '<em>two</em>'}>html</button>
