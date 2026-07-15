<script>
    let promise = Promise.resolve('ready');
</script>
{#await promise}<p>pending</p>{:then value}<p>{value}</p>{/await}
