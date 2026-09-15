<script lang="ts">
  const label = "outer";
  function inner() {
    const label = 1;
    const n: number = label;
    void n;
  }
  void inner;
  void label;
</script>

<p>ok</p>
