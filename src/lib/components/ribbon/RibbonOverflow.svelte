<script lang="ts">
  // Basitleştirilmiş şeridin "…" listesi: şeridin içeriği burada bir kez daha çizilir;
  // yalnızca şeritte yer kalmadığı için gizlenen (sağdaki) gruplar görünür.
  import { setContext, type Snippet } from "svelte";
  let { cut, children }: { cut: number; children: Snippet } = $props();
  setContext("ribbon", {
    overflow: true,
    get cut() {
      return cut;
    },
    groups: 0,
  });
</script>

<div class="ribbon-panel ribbon-overflow flex flex-col" data-density="simple">{@render children()}</div>
