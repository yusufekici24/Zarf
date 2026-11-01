<script lang="ts">
  // Sol bölmedeki küçük ay takvimi (Outlook Tarih Gezgini): etkinlikli günler kalın,
  // görüntülenen aralık vurgulu, bugün mavi kare.
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { addDays, sameDay, startOfDay, startOfWeek, weekdayShort } from "$lib/format";
  import { i18n } from "$lib/i18n.svelte";

  let {
    month,
    rangeStart = null,
    rangeDays = 0,
    busyDays = new Set<string>(),
    onpick,
    onprev,
    onnext,
  }: {
    /** Ayın herhangi bir günü */
    month: Date;
    rangeStart?: Date | null;
    rangeDays?: number;
    busyDays?: Set<string>;
    onpick: (d: Date) => void;
    onprev?: () => void;
    onnext?: () => void;
  } = $props();

  const first = $derived(new Date(month.getFullYear(), month.getMonth(), 1));
  const days = $derived.by(() => {
    const s = startOfWeek(first);
    return Array.from({ length: 42 }, (_, i) => addDays(s, i));
  });
  // Son satır tamamen sonraki aydaysa gösterilmez (Outlook gibi 5 ya da 6 satır)
  const rows = $derived(days[35].getMonth() !== first.getMonth() ? 5 : 6);
  const today = startOfDay(new Date());
  // Outlook Türkçe kısaltmaları
  const TR = ["PA", "PT", "SA", "ÇA", "PE", "CU", "CT"];
  function wd(d: Date): string {
    return i18n.locale.startsWith("tr") ? TR[d.getDay()] : weekdayShort(d).slice(0, 2);
  }
  const rs = $derived(rangeStart ? startOfDay(rangeStart).getTime() : 0);
  const re = $derived(rangeStart ? addDays(startOfDay(rangeStart), rangeDays).getTime() : 0);
</script>

<div class="px-3 pt-2 pb-1 text-[12px] select-none">
  <div class="mb-1 flex h-6 items-center justify-between">
    {#if onprev}<button class="hover:bg-accent rounded p-0.5" aria-label="‹" onclick={onprev}><ChevronLeft class="size-4" /></button>{:else}<span class="w-5"></span>{/if}
    <span class="font-semibold capitalize">{first.toLocaleDateString(i18n.locale, { month: "long", year: "numeric" })}</span>
    {#if onnext}<button class="hover:bg-accent rounded p-0.5" aria-label="›" onclick={onnext}><ChevronRight class="size-4" /></button>{:else}<span class="w-5"></span>{/if}
  </div>
  <div class="grid grid-cols-7 text-center">
    {#each days.slice(0, 7) as d (d.getTime())}<div class="text-muted-foreground py-0.5 text-[11px] uppercase">{wd(d)}</div>{/each}
    {#each days.slice(0, rows * 7) as d (d.getTime())}
      {@const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`}
      {@const inRange = d.getTime() >= rs && d.getTime() < re}
      {@const isToday = sameDay(d, today)}
      {@const other = d.getMonth() !== first.getMonth()}
      <button
        class="h-6 text-[12px] {isToday ? 'bg-primary font-semibold text-white' : inRange ? 'bg-[#cfe4fa] dark:bg-primary/30' : 'hover:bg-[#e5f1fb] dark:hover:bg-white/10'} {other && !isToday ? 'text-muted-foreground/70' : ''} {busyDays.has(key) && !other ? 'font-bold' : ''}"
        onclick={() => onpick(d)}
      >
        {d.getDate()}
      </button>
    {/each}
  </div>
</div>
