<script module lang="ts">
  export const moduleConst = "from-module";
</script>

<script lang="ts">
  const localMirror = "from-module";
  let value = $state(localMirror);
</script>

<p>{value}</p>
