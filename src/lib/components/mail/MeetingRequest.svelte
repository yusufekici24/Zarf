<script lang="ts">
  // Outlook okuma bölmesindeki toplantı isteği: Kabul Et / Belki / Reddet (her biri "Yanıtı
  // Şimdi Gönder", "Göndermeden Önce Yanıtı Düzenle", "Yanıt Gönderme"), Yeni Saat Öner,
  // Takvim...; tarih, yer, katılımcılar, takvimimle çakışma bilgisi ve o günün kısa görünümü.
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import DateField from "$lib/components/common/DateField.svelte";
  import TimeField from "$lib/components/common/TimeField.svelte";
  import RecipientLine from "$lib/components/mail/RecipientLine.svelte";
  import { api, type MeetingOcc } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { IS_MAIN } from "$lib/window";
  import { emitTo } from "@tauri-apps/api/event";
  import { i18n, t } from "$lib/i18n.svelte";
  import { startOfDay, toDate, toInputTime, toTs } from "$lib/format";
  import { toast } from "svelte-sonner";
  import Check from "@lucide/svelte/icons/check";
  import HelpCircle from "@lucide/svelte/icons/circle-help";
  import X from "@lucide/svelte/icons/x";
  import Clock from "@lucide/svelte/icons/clock";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import InfoIcon from "@lucide/svelte/icons/info";
  import AlertTriangle from "@lucide/svelte/icons/triangle-alert";
  import Trash from "@lucide/svelte/icons/trash-2";

  import type { Snippet } from "svelte";

  let { id, organizerView = false, fallback }: { id: number; organizerView?: boolean; /** Toplantı isteği değilse (ör. yayımlanan .ics) eski çubuk */ fallback?: Snippet } = $props();

  type Info = Awaited<ReturnType<typeof api.meetingInfo>>;
  let info = $state<Info | null>(null);
  let failed = $state(false);
  $effect(() => {
    const my = id;
    info = null;
    failed = false;
    api.meetingInfo(my)
      .then((r) => {
        if (my === id) info = r;
      })
      .catch(() => {
        if (my === id) failed = true;
      });
  });
  // Yanıt verilince takvim değişir: bilgi (yanıtım, çakışmalar) yenilenir
  $effect(() => {
    void app.revision.calendar;
    const my = id;
    if (info) void api.meetingInfo(my).then((r) => my === id && (info = r)).catch(() => {});
  });

  type Kind = "accept" | "tentative" | "decline";
  async function respond(kind: Kind, mode: "send" | "edit" | "none", comment = "") {
    if (mode === "edit") {
      const c = await app.prompt(t(`meeting.${kind}`), t("meeting.comment_label"), comment);
      if (c == null) return;
      comment = c;
    }
    try {
      await api.respondMeeting(id, kind, comment, mode !== "none");
      toast.success(t(mode === "none" ? "meeting.updated_no_reply" : `meeting.${kind}_done`));
      if (info) info = { ...info, response: kind };
      app.bump("calendar");
    } catch (e) {
      app.error(e);
    }
  }

  /** "Takvim...": ana pencerede takvim o güne açılır */
  async function showCalendar(ts: number) {
    if (IS_MAIN) {
      app.calendarGoto = ts;
      app.module = "calendar";
      return;
    }
    await emitTo("main", "app://calendar-goto", ts).catch(() => {});
    await api.focusMain().catch(() => {});
  }

  // ------------------------------------------------------------ Yeni Saat Öner
  let propose = $state<{ kind: Kind; date: number | null; start: string; end: string } | null>(null);
  function openPropose(kind: Kind) {
    if (!info) return;
    propose = { kind, date: toTs(startOfDay(toDate(info.start))), start: toInputTime(info.start), end: toInputTime(info.end) };
  }
  async function sendPropose() {
    const p = propose;
    if (!p || p.date == null) return;
    const at = (h: string) => p.date! + (Number(h.split(":")[0]) * 60 + Number(h.split(":")[1] || 0)) * 60;
    propose = null;
    await respond(p.kind, "send", t("meeting.proposed_text", range(at(p.start), at(p.end), false)));
  }

  function range(s: number, e: number, allDay: boolean): string {
    const a = new Date(s * 1000);
    const b = new Date(e * 1000);
    const d = (x: Date) => x.toLocaleDateString(i18n.locale, { weekday: "short", day: "2-digit", month: "2-digit", year: "numeric" });
    const h = (x: Date) => x.toLocaleTimeString(i18n.locale, { hour: "2-digit", minute: "2-digit" });
    if (allDay) return d(a);
    return a.toDateString() === b.toDateString() ? `${d(a)} ${h(a)} - ${h(b)}` : `${d(a)} ${h(a)} - ${d(b)} ${h(b)}`;
  }
  const hm = (ts: number) => new Date(ts * 1000).toLocaleTimeString(i18n.locale, { hour: "2-digit", minute: "2-digit" });

  // ------------------------------------------------------------ günün kısa görünümü (yatay)
  const preview = $derived.by(() => {
    if (!info || info.all_day) return null;
    const d0 = toTs(startOfDay(toDate(info.start)));
    const h0 = Math.max(0, Math.floor((info.start - d0) / 3600) - 2);
    const h1 = Math.min(24, Math.max(h0 + 6, Math.ceil((info.end - d0) / 3600) + 2));
    const from = d0 + h0 * 3600;
    const to = d0 + h1 * 3600;
    const pct = (ts: number) => ((Math.min(Math.max(ts, from), to) - from) / (to - from)) * 100;
    const items = info.day.filter((o) => !o.all_day && o.start < to && o.end > from);
    return { h0, h1, pct, items };
  });
  const stateClass = (o: MeetingOcc) => (o.show_as === "tentative" ? "border-sky-400 bg-sky-100 dark:bg-sky-950" : o.show_as === "oof" ? "border-purple-500 bg-purple-100 dark:bg-purple-950" : "border-[#0f6cbd] bg-[#dbe8f7] dark:bg-[#1b3550]");

  const btn = "flex h-7 items-center gap-1.5 rounded-[3px] px-2 text-[13px] hover:bg-black/5 dark:hover:bg-white/10";
</script>

{#snippet respondMenu(kind: Kind, Icon: typeof Check, color: string)}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger class={btn}>
      <Icon class="size-4 {color}" />{t(`meeting.${kind}`)}<ChevronDown class="size-3 opacity-60" />
    </DropdownMenu.Trigger>
    <DropdownMenu.Content align="start" class="min-w-64 whitespace-nowrap">
      <DropdownMenu.Item onclick={() => respond(kind, "edit")}>{t("meeting.reply_edit")}</DropdownMenu.Item>
      <DropdownMenu.Item onclick={() => respond(kind, "send")}>{t("meeting.reply_now")}</DropdownMenu.Item>
      <DropdownMenu.Item onclick={() => respond(kind, "none")}>{t("meeting.reply_none")}</DropdownMenu.Item>
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/snippet}

{#if info && !["REQUEST", "CANCEL", ""].includes(info.method.toUpperCase())}
  {@render fallback?.()}
{:else if info}
  <div class="flex flex-col gap-2 text-[13px]">
    <!-- Katılımcılar, tarih/yer/çakışma, yanıt düğmeleri, sonra günün çizelgesi -->
    {#if info.attendees || info.optional || info.resources}
      <div>
        {#if info.attendees}<RecipientLine label={t("calendar.required")} value={[info.attendees, info.resources].filter(Boolean).join(",")} />{/if}
        {#if info.optional}<RecipientLine label={t("calendar.optional")} value={info.optional} />{/if}
      </div>
    {/if}
    <div>
      <div class="min-w-0">
        <div class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
          <span class="text-muted-foreground font-semibold">{t("meeting.when")}</span>
          <span class="font-semibold">{range(info.start, info.end, info.all_day)}{info.recurrence !== "none" ? ` (${t(`repeat.${info.recurrence}`)})` : ""}</span>
          <span class="text-muted-foreground font-semibold">{t("meeting.where")}</span>
          <span class="font-semibold break-words">{info.location}</span>
        </div>
        <!-- Takvimimle çakışma (Outlook: "Çakışma yok") -->
        <div class="mt-2 flex items-start gap-1.5">
          {#if info.conflicts.length}
            <AlertTriangle class="mt-0.5 size-4 shrink-0 text-amber-600" />
            <span>{info.conflicts.length === 1 ? t("meeting.conflict_one", info.conflicts[0].title || t("showas.busy")) : t("meeting.conflict_n", info.conflicts.length)}</span>
          {:else if info.adjacent.length}
            <InfoIcon class="text-muted-foreground mt-0.5 size-4 shrink-0" />
            <span>{t("meeting.adjacent", info.adjacent[0].title || t("showas.busy"))}</span>
          {:else}
            <InfoIcon class="text-muted-foreground mt-0.5 size-4 shrink-0" />
            <span class="font-semibold">{t("meeting.no_conflict")}</span>
          {/if}
        </div>
      </div>

    </div>
    {#if info.cancelled}
      <div class="flex items-center gap-2 rounded-[3px] bg-red-50 px-3 py-1.5 text-red-900 dark:bg-red-950 dark:text-red-100">
        <AlertTriangle class="size-4 shrink-0" />
        <span class="flex-1">{t("meeting.cancelled_notice")}</span>
        {#if info.event_id}
          <button class={btn} onclick={async () => { await api.deleteEvent(info!.event_id!, "series"); app.bump("calendar"); toast.success(t("meeting.removed")); }}><Trash class="size-4" />{t("meeting.remove_from_calendar")}</button>
        {/if}
      </div>
    {:else if !organizerView}
      <!-- Yanıt düğmeleri (Outlook şeridi gibi) -->
      <div class="flex flex-wrap items-center gap-0.5 rounded-[3px] border px-1 py-0.5">
        {@render respondMenu("accept", Check, "text-emerald-600")}
        {@render respondMenu("tentative", HelpCircle, "text-violet-600")}
        {@render respondMenu("decline", X, "text-red-600")}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger class={btn}><Clock class="size-4 text-[#0f6cbd]" />{t("meeting.propose")}<ChevronDown class="size-3 opacity-60" /></DropdownMenu.Trigger>
          <DropdownMenu.Content align="start" class="min-w-64 whitespace-nowrap">
            <DropdownMenu.Item onclick={() => openPropose("tentative")}>{t("meeting.propose_tentative")}</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => openPropose("decline")}>{t("meeting.propose_decline")}</DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Root>
        <button class={btn} onclick={() => showCalendar(info!.start)}><CalendarIcon class="size-4" />{t("meeting.calendar")}</button>
      </div>
    {/if}
    {#if info.response && info.response !== "none" && info.response !== "organizer" && !info.cancelled}
      <div class="text-muted-foreground">{t(`meeting.you_${info.response}`)}</div>
    {/if}

    {#if preview}
      <!-- O günün kısa görünümü (yatay zaman çizelgesi): toplantı ve takvimimdeki öğeler -->
      <div class="overflow-x-auto rounded-[3px] border">
        <div class="relative h-[92px] min-w-[480px]">
          {#each Array(preview.h1 - preview.h0) as _, i (i)}
            <div class="absolute top-0 bottom-0 border-l {i === 0 ? 'border-l-0' : ''}" style="left: {(i / (preview.h1 - preview.h0)) * 100}%; width: {100 / (preview.h1 - preview.h0)}%">
              <span class="text-muted-foreground absolute top-0.5 left-1 text-[10px]">{String(preview.h0 + i).padStart(2, "0")}:00</span>
              <div class="absolute top-[18px] bottom-0 left-1/2 border-l border-dotted opacity-40"></div>
            </div>
          {/each}
          <div class="absolute top-[19px] h-[36px] overflow-hidden rounded-[2px] border-2 border-dashed border-[#0f6cbd] bg-white/85 px-1.5 py-0.5 text-[11px] leading-[14px] font-semibold dark:bg-black/40" style="left: {preview.pct(info.start)}%; width: {Math.max(4, preview.pct(info.end) - preview.pct(info.start))}%" title="{info.title} {hm(info.start)} - {hm(info.end)}">
            <div class="truncate">{info.title}</div>
            <div class="text-muted-foreground truncate font-normal">{hm(info.start)} - {hm(info.end)}</div>
          </div>
          {#each preview.items as o, k (k)}
            <div class="absolute top-[60px] h-6 overflow-hidden rounded-[2px] border-l-4 px-1 text-[11px] leading-6 {stateClass(o)}" style="left: {preview.pct(o.start)}%; width: {Math.max(2, preview.pct(o.end) - preview.pct(o.start))}%" title="{o.title} {hm(o.start)}-{hm(o.end)}">
              <span class="truncate">{o.title}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
{:else if failed}
  {@render fallback?.()}
{/if}

<Dialog.Root open={propose != null} onOpenChange={(o) => !o && (propose = null)}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header><Dialog.Title>{t("meeting.propose")}</Dialog.Title></Dialog.Header>
    {#if propose}
      <div class="grid grid-cols-[80px_1fr] items-center gap-2 text-sm">
        <span>{t("calendar.start")}</span>
        <div class="flex items-center gap-2"><DateField bind:value={propose.date} /><TimeField bind:value={propose.start} /></div>
        <span>{t("calendar.end")}</span>
        <div class="flex items-center gap-2"><TimeField bind:value={propose.end} /></div>
      </div>
    {/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (propose = null)}>{t("common.cancel")}</Button>
      <Button onclick={sendPropose}>{t("meeting.propose_send")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
