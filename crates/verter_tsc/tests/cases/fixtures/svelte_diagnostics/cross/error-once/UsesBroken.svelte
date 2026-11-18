<script lang="ts">
  import { h } from "vue";
  import Broken from "./Broken.vue";
  const vnode = h(Broken, { label: "ok" });
</script>

<p>{String(vnode)}</p>
