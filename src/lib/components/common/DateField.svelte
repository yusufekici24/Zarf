<script lang="ts">
  // Tarih seçici: metin düğmesi + açılır takvim. Değer: günün yerel 00:00 zaman damgası (sn).
  import * as Popover from "$lib/components/ui/popover";
  import { Calendar } from "$lib/components/ui/calendar";
  import { Button } from "$lib/components/ui/button";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import XIcon from "@lucide/svelte/icons/x";
  import { CalendarDate, type DateValue } from "@internationalized/date";
  import { i18n, t } from "$lib/i18n.svelte";
  import { fmtDate, toDate, toTs } from "$lib/format";

  let {
    value = $bindable<number | null>(null),
    clearable = false,
    disabled = false,
    class: className = "",
    onchange,
  }: {
    value?: number | null;
    clearable?: boolean;
    disabled?: boolean;
    class?: string;
    onchange?: (v: number | null) => void;
  } = $props();

  let open = $state(false);

  const calValue = $derived.by(() => {
    if (!value) return undefined;
    const d = toDate(value);
    return new CalendarDate(d.getFullYear(), d.getMonth() + 1, d.getDate());
  });

  function pick(v: DateValue | undefined) {
    if (!v) return;
    // Varsa mevcut saati koru
    const old = value ? toDate(value) : null;
    const d = new Date(v.year, v.month - 1, v.day, old?.getHours() ?? 0, old?.getMinutes() ?? 0);
    value = toTs(d);
    onchange?.(value);
    open = false;
  }
</script>

<div class="flex items-center gap-1 {className}">
  <Popover.Root bind:open>
    <Popover.Trigger {disabled}>
      {#snippet child({ props })}
        <Button {...props} variant="outline" size="sm" class="h-9 min-w-36 justify-start font-normal" {disabled}>
          <CalendarIcon class="size-4 opacity-60" />
          {value ? fmtDate(value, { weekday: "short", day: "numeric", month: "short", year: "numeric" }) : t("common.none")}
        </Button>
      {/snippet}
    </Popover.Trigger>
    <Popover.Content class="w-auto p-0" align="start">
      <Calendar type="single" value={calValue} onValueChange={pick} locale={i18n.locale} weekStartsOn={1} captionLayout="dropdown" />
      <div class="flex gap-1 border-t p-2">
        <Button size="sm" variant="ghost" class="flex-1" onclick={() => { const n = new Date(); pick(new CalendarDate(n.getFullYear(), n.getMonth() + 1, n.getDate())); }}>{t("common.today")}</Button>
        {#if clearable}
          <Button size="sm" variant="ghost" class="flex-1" onclick={() => { value = null; onchange?.(null); open = false; }}>{t("common.none")}</Button>
        {/if}
      </div>
    </Popover.Content>
  </Popover.Root>
  {#if clearable && value}
    <Button variant="ghost" size="icon-sm" onclick={() => { value = null; onchange?.(null); }} title={t("common.clear")}><XIcon class="size-4" /></Button>
  {/if}
</div>
