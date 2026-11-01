<script lang="ts">
  // Saat seçici (HH:MM): yazılabilir alan + 30 dakikalık öneri listesi.
  let {
    value = $bindable("09:00"),
    disabled = false,
    onchange,
  }: { value?: string; disabled?: boolean; onchange?: (v: string) => void } = $props();

  const id = `times-${Math.random().toString(36).slice(2)}`;
  const options: string[] = [];
  for (let h = 0; h < 24; h++) for (const m of [0, 30]) options.push(`${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`);

  function normalize(v: string): string {
    const m = /^(\d{1,2})[:.]?(\d{0,2})$/.exec(v.trim());
    if (!m) return value;
    const h = Math.min(23, Number(m[1]));
    const mm = Math.min(59, Number(m[2] || 0));
    return `${String(h).padStart(2, "0")}:${String(mm).padStart(2, "0")}`;
  }
</script>

<input
  list={id}
  class="border-input dark:bg-input/30 h-9 w-24 rounded-md border bg-transparent px-2 text-sm shadow-xs outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50"
  {disabled}
  {value}
  onchange={(e) => { value = normalize((e.target as HTMLInputElement).value); (e.target as HTMLInputElement).value = value; onchange?.(value); }}
/>
<datalist {id}>
  {#each options as o (o)}<option value={o}></option>{/each}
</datalist>
