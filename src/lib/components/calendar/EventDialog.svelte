<script lang="ts">
  // Randevu / toplantı düzenleme penceresi.
  import * as Dialog from "$lib/components/ui/dialog";
  import { untrack } from "svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Textarea } from "$lib/components/ui/textarea";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import DateField from "$lib/components/common/DateField.svelte";
  import TimeField from "$lib/components/common/TimeField.svelte";
  import AddressInput, { roomCache } from "$lib/components/common/AddressInput.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { i18n, t } from "$lib/i18n.svelte";
  import { fold, splitAddresses, splitNameAddr, startOfDay, toDate, toInputTime, toTs } from "$lib/format";
  import type { CalEvent, Calendar } from "$lib/types";
  import { save as saveDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";
  import WindowTitleBar from "$lib/components/common/WindowTitleBar.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Send from "@lucide/svelte/icons/send";
  import SaveIcon from "@lucide/svelte/icons/save";
  import Trash from "@lucide/svelte/icons/trash-2";
  import CalendarClock from "@lucide/svelte/icons/calendar-clock";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import DoorOpen from "@lucide/svelte/icons/door-open";
  import UserIcon from "@lucide/svelte/icons/user";

  let {
    open = $bindable(false),
    event,
    occurrenceStart = null,
    calendars,
    onsaved,
    readonly = false,
    windowed = false,
    meeting = false,
  }: {
    open?: boolean;
    event: CalEvent | null;
    occurrenceStart?: number | null;
    calendars: Calendar[];
    onsaved: () => void;
    /** Başkasının / internet takvimi: salt okunur görünüm (paylaşılan izin kadar ayrıntı) */
    readonly?: boolean;
    /** Ayrı pencerede (Outlook gibi): başlık çubuğu, "Toplantı" ve "Zamanlama Yardımcısı" sekmeleri */
    windowed?: boolean;
    /** "Yeni Toplantı" ile açıldı */
    meeting?: boolean;
  } = $props();

  // Salt okunur görünüm bilgileri
  const roCal = $derived(calendars.find((c) => c.id === event?.calendar_id));
  /** Yalnızca boş/meşgul paylaşılmış: konu yerine durum adı gelir, kimlik yoktur */
  const onlyFreeBusy = $derived(!!event && !event.remote_id && !event.location && !event.description && !event.organizer && event.title === t(`showas.${event.show_as}`));
  const fmtRange = (ev: CalEvent) => {
    const s = new Date((occurrenceStart ?? ev.start) * 1000);
    const en = new Date((occurrenceStart != null ? occurrenceStart + (ev.end - ev.start) : ev.end) * 1000);
    const d = (x: Date) => x.toLocaleDateString(i18n.locale, { weekday: "short", day: "numeric", month: "long", year: "numeric" });
    const h = (x: Date) => x.toLocaleTimeString(i18n.locale, { hour: "2-digit", minute: "2-digit" });
    if (ev.all_day) return d(s);
    return s.toDateString() === en.toDateString() ? `${d(s)} ${h(s)} - ${h(en)}` : `${d(s)} ${h(s)} - ${d(en)} ${h(en)}`;
  };
  const list = (v?: string) => (v ?? "").split(/[,;]/).map((x) => x.trim()).filter(Boolean);

  let e = $state<CalEvent>(blank());
  let startDate = $state<number | null>(null);
  let endDate = $state<number | null>(null);
  let startTime = $state("09:00");
  let endTime = $state("09:30");
  let attendees = $state("");
  let optional = $state("");
  let rooms = $state("");
  let showOptional = $state(false);
  let untilDate = $state<number | null>(null);

  // ---------------------------------------------------------------- Oda Bulucu
  let roomList = $state<{ list: string; name: string; email: string }[]>([]);
  let roomOpen = $state(false);
  let roomQuery = $state("");
  let roomsLoading = $state(false);
  async function openRooms() {
    roomOpen = !roomOpen;
    if (roomOpen && !roomList.length) {
      roomsLoading = true;
      roomList = await roomCache();
      roomsLoading = false;
    }
  }
  /** Toplantının başlangıç/bitişi (saniye) */
  const toMin = (h: string) => Number(h.split(":")[0]) * 60 + Number(h.split(":")[1] || 0);
  const meetingRange = $derived<[number, number] | null>(
    startDate == null ? null : [startDate + (e.all_day ? 0 : toMin(startTime) * 60), (endDate ?? startDate) + (e.all_day ? 86400 : toMin(endTime) * 60)],
  );
  /** Oda Bulucu: seçili saatte odaların durumu (Outlook gibi müsait odalar önce) */
  let roomFb = $state<Record<string, "free" | "busy" | "unknown">>({});
  let roomFbKey = "";
  let onlyFree = $state(false);
  $effect(() => {
    const r = meetingRange;
    if (!roomOpen || !r || !roomList.length || r[1] <= r[0]) return;
    const key = `${r[0]}-${r[1]}-${roomList.length}`;
    if (key === roomFbKey) return;
    roomFbKey = key;
    const emails = roomList.map((x) => x.email);
    const dur = Math.max(5, Math.min(1440, Math.round((r[1] - r[0]) / 60)));
    untrack(() => (roomFb = {}));
    void api
      .freeBusy(emails, r[0], r[1], dur)
      .then((m) => {
        if (key !== roomFbKey) return;
        const out: Record<string, "free" | "busy" | "unknown"> = {};
        for (const em of emails) {
          const v = (m as Record<string, string>)[em] ?? "";
          out[em] = /[123]/.test(v) ? "busy" : /^0+$/.test(v) ? "free" : "unknown";
        }
        roomFb = out;
      })
      .catch(() => {});
  });
  const roomGroups = $derived.by(() => {
    const words = fold(roomQuery.trim()).split(/\s+/).filter(Boolean);
    const m = new Map<string, { name: string; email: string }[]>();
    const rank = (em: string) => (roomFb[em] === "free" ? 0 : roomFb[em] === "busy" ? 2 : 1);
    for (const r of [...roomList].sort((a, b) => rank(a.email) - rank(b.email))) {
      const hay = fold(`${r.name} ${r.email}`);
      if (words.some((w) => !hay.includes(w))) continue;
      if (onlyFree && roomFb[r.email] !== "free") continue;
      if (!m.has(r.list)) m.set(r.list, []);
      m.get(r.list)!.push(r);
    }
    return [...m.entries()];
  });
  // Oda listelerinde/adres defterinde bulunmayan oda: kurum dizininde sunucuda canlı aranır
  // (Outlook "Oda Seç: Tüm Odalar" araması gibi), bulunanlar listeye eklenir
  let dirRooms = $state<{ name: string; email: string }[]>([]);
  let dirTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const q = roomQuery.trim();
    clearTimeout(dirTimer);
    if (!roomOpen || q.length < 2) {
      dirRooms = [];
      return;
    }
    dirTimer = setTimeout(() => {
      void api
        .listRooms(q)
        .then((list) => {
          if (roomQuery.trim() !== q) return;
          const known = new Set(roomList.map((r) => r.email.toLowerCase()));
          const added = list.filter((r) => !known.has(r.email.toLowerCase()));
          if (added.length) roomList = [...roomList, ...added];
          dirRooms = [];
        })
        .catch(() => (dirRooms = []));
    }, 300);
  });

  function addRoom(r: { name: string; email: string }) {
    const list = splitAddresses(rooms);
    if (!list.some((x) => splitNameAddr(x).addr.toLowerCase() === r.email.toLowerCase())) list.push(`${r.name} <${r.email}>`);
    rooms = list.join(", ");
    // Outlook: konum oda adlarından oluşur
    const names = splitAddresses(rooms).map((x) => splitNameAddr(x).name || splitNameAddr(x).addr);
    if (!e.location.trim() || roomNames.includes(e.location)) e.location = names.join("; ");
    roomNames = e.location;
    // Outlook gibi: oda seçilince Zamanlama Yardımcısı açılır, odanın ve katılımcıların durumu görünür
    schedOpen = true;
  }
  let roomNames = "";

  // ---------------------------------------------------------------- Zamanlama Yardımcısı
  let schedOpen = $state(false);
  /** Ayrı pencerede sekme: "main" (Toplantı) | "sched" (Zamanlama Yardımcısı) */
  let tab = $state<"main" | "sched">("main");
  type FbEvent = { start: number; end: number; show_as: string; title: string; location: string };
  let fb = $state<Record<string, string>>({});
  let fbEvents = $state<Record<string, FbEvent[]>>({});
  let fbLoading = $state(false);
  let fbError = $state("");
  /** Yalnızca çalışma saatleri (Outlook varsayılanı) ya da tüm gün */
  let workOnly = $state(true);
  const WORK_START = $derived(workOnly ? 8 : 0);
  const WORK_END = $derived(workOnly ? 19 : 24);
  const SLOT = 30;
  const people = $derived([
    ...splitAddresses(attendees).map((a) => ({ ...splitNameAddr(a), role: "req" })),
    ...splitAddresses(optional).map((a) => ({ ...splitNameAddr(a), role: "opt" })),
    ...splitAddresses(rooms).map((a) => ({ ...splitNameAddr(a), role: "room" })),
  ].filter((a) => a.addr));
  let fbKey = "";
  async function loadFreeBusy(force = false) {
    if (!people.length || startDate == null) return;
    const day = startDate;
    const emails = people.map((p) => p.addr);
    const key = `${day}|${emails.join(",")}`;
    if (key === fbKey && !force) return;
    fbKey = key;
    fbLoading = true;
    fbError = "";
    try {
      // Tüm gün istenir: çalışma saatleri / tüm gün arasında geçişte yeniden sorulmaz
      const r = await api.freeBusyDetails(emails, day, day + 86400, SLOT);
      if (key !== fbKey) return;
      fb = Object.fromEntries(Object.entries(r).map(([k, v]) => [k, v.merged]));
      fbEvents = Object.fromEntries(Object.entries(r).map(([k, v]) => [k, v.events]));
    } catch (err) {
      if (key !== fbKey) return;
      fbError = String(err);
      fb = {};
      fbEvents = {};
    } finally {
      if (key === fbKey) fbLoading = false;
    }
  }
  // Toplantı çalışma saatleri dışındaysa tüm gün gösterilir
  $effect(() => {
    if (tab !== "sched") return;
    const [a, b] = [toMin(startTime), toMin(endTime)];
    untrack(() => {
      if (a < 8 * 60 || b > 19 * 60) workOnly = false;
    });
  });
  $effect(() => {
    if (schedOpen || tab === "sched") {
      void people.length;
      void startDate;
      void loadFreeBusy();
    }
  });
  const slotCount = $derived(((WORK_END - WORK_START) * 60) / SLOT);
  const FB_CLASS = ["bg-transparent", "bg-sky-300", "bg-blue-600", "bg-purple-600", "bg-[repeating-linear-gradient(45deg,#ccc_0,#ccc_2px,transparent_2px,transparent_5px)]"];
  /** Kişinin o dilimdeki durumu (birleşik dizi tüm günü 30 dk dilimlerle verir) */
  function stateAt(addr: string, i: number): number {
    const s = fb[addr];
    if (!s) return fbLoading ? 0 : 4;
    return Number(s[(WORK_START * 60) / SLOT + i] ?? 4);
  }
  /** "Tüm Katılımcılar" satırı: dilimdeki en yoğun durum (bilgi yok sayılmaz) */
  function allAt(i: number): number {
    let m = 0;
    for (const p of people) {
      const v = stateAt(p.addr, i);
      if (v !== 4 && v > m) m = v;
    }
    return m;
  }
  /** Zamanlama Yardımcısı: gün değiştir (toplantı süresi korunur) */
  function shiftDay(days: number) {
    if (startDate == null) return;
    const span = (endDate ?? startDate) - startDate;
    startDate += days * 86400;
    endDate = startDate + span;
  }
  /** Zamanlama Yardımcısı: boş bir dilime tıklayınca toplantı o saate taşınır (süre korunur) */
  function pickSlot(i: number) {
    const dur = Math.max(SLOT, toMin(endTime) - toMin(startTime));
    const s = WORK_START * 60 + i * SLOT;
    const en = s + dur;
    const fmt = (m: number) => `${String(Math.floor(m / 60) % 24).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
    startTime = fmt(s);
    endTime = fmt(en);
    if (en >= 24 * 60 && startDate != null) endDate = startDate + 86400;
    else endDate = startDate;
  }
  /** Sonraki uygun saat (Outlook "Otomatik Seç"): tüm katılımcıların boş olduğu ilk dilim */
  function autoPick(dir: 1 | -1) {
    const dur = Math.max(1, Math.round((toMin(endTime) - toMin(startTime)) / SLOT));
    const [cur] = meetingSlots();
    for (let i = Math.round(cur) + dir; i >= 0 && i + dur <= slotCount; i += dir) {
      let ok = true;
      for (let k = 0; k < dur && ok; k++) if (allAt(i + k) !== 0) ok = false;
      if (ok) return pickSlot(i);
    }
    toast.info(t("calendar.no_free_slot"));
  }
  /** Öğenin dilim ızgarasındaki yeri (yüzde) */
  function blockPos(ev: FbEvent): { left: number; width: number } | null {
    if (startDate == null) return null;
    const a = startDate + WORK_START * 3600;
    const b = startDate + WORK_END * 3600;
    const s = Math.max(ev.start, a);
    const en = Math.min(ev.end, b);
    if (en <= s) return null;
    return { left: ((s - a) / (b - a)) * 100, width: ((en - s) / (b - a)) * 100 };
  }
  function meetingSlots(): [number, number] {
    const s = (Number(startTime.split(":")[0]) * 60 + Number(startTime.split(":")[1]) - WORK_START * 60) / SLOT;
    const en = (Number(endTime.split(":")[0]) * 60 + Number(endTime.split(":")[1]) - WORK_START * 60) / SLOT;
    return [s, en];
  }
  /** Toplantı mı (katılımcı/oda var ya da "Yeni Toplantı" ile açıldı): pencere adı */
  const isMeeting = $derived(meeting || !!(attendees.trim() || optional.trim() || rooms.trim()));
  const windowTitle = $derived(
    `${e.title.trim() || t("calendar.untitled_window")} - ${isMeeting ? t("calendar.meeting") : t("calendar.appointment")}`,
  );
  $effect(() => {
    if (windowed) void getCurrentWindow().setTitle(windowTitle).catch(() => {});
  });
  function closeWindow() {
    void getCurrentWindow().destroy().catch(() => window.close());
  }

  function blank(): CalEvent {
    return {
      id: 0, calendar_id: 1, uid: "", title: "", location: "", description: "", start: 0, end: 0, all_day: false,
      recurrence: "none", recur_interval: 1, recur_until: null, exdates: "", reminder_minutes: Number(app.setting("default_reminder", "15")),
      category: "", show_as: "busy", attendees: "", is_private: false,
    };
  }

  // Açılınca bir kez doldurulur: yalnızca open/event/occurrenceStart izlenir. Gövde
  // izlenmez; yoksa yazdığı state'i (e, optional) okuyup sonsuz döngüye giriyordu
  // ("Maximum update depth exceeded" → pencere bozuk/boş görünüyordu).
  $effect(() => {
    if (!open || !event) return;
    const ev = event;
    void occurrenceStart;
    untrack(() => fill(ev));
  });

  function fill(event: CalEvent) {
    e = { ...blank(), ...event };
    const s = occurrenceStart ?? e.start;
    const dur = e.end - e.start;
    startDate = toTs(startOfDay(toDate(s)));
    endDate = toTs(startOfDay(toDate(s + dur - (e.all_day ? 1 : 0))));
    startTime = toInputTime(s);
    endTime = toInputTime(s + dur);
    attendees = e.attendees.split(",").filter(Boolean).join(", ");
    optional = (e.optional_attendees ?? "").split(",").filter(Boolean).join(", ");
    rooms = (e.resources ?? "").split(",").filter(Boolean).join(", ");
    showOptional = !!optional;
    roomNames = "";
    schedOpen = false;
    fb = {};
    untilDate = e.recur_until;
    if (!e.calendar_id && calendars[0]) e.calendar_id = calendars[0].id;
  }

  function combine(day: number | null, time: string): number {
    const d = toDate(day ?? toTs(new Date()));
    const [h, m] = time.split(":").map(Number);
    d.setHours(e.all_day ? 0 : h, e.all_day ? 0 : m, 0, 0);
    return toTs(d);
  }

  function build(): CalEvent {
    let start = combine(startDate, startTime);
    let end = combine(endDate, endTime);
    if (e.all_day) end = toTs(startOfDay(toDate(end))) + 86400;
    if (end <= start) end = start + (e.all_day ? 86400 : 1800);
    const ev: CalEvent = {
      ...e,
      start,
      end,
      attendees: splitAddresses(attendees).map((a) => splitNameAddr(a).addr).filter(Boolean).join(","),
      optional_attendees: splitAddresses(optional).map((a) => splitNameAddr(a).addr).filter(Boolean).join(","),
      resources: splitAddresses(rooms).map((a) => splitNameAddr(a).addr).filter(Boolean).join(","),
      recur_until: e.recurrence === "none" ? null : untilDate ? untilDate + 86399 : null,
    };
    // Tek bir yinelemeyi düzenlerken seri başlangıcını koru
    if (occurrenceStart != null && event && event.recurrence !== "none") {
      const delta = start - occurrenceStart;
      ev.start = event.start + delta;
      ev.end = ev.start + (end - start);
    }
    return ev;
  }

  async function save(invite = false) {
    if (!e.title.trim()) e.title = t("calendar.untitled");
    try {
      const id = await api.saveEvent(build());
      if (invite) {
        const acc = app.defaultAccountId();
        if (!acc) throw t("compose.no_account");
        await api.sendInvitation(id, acc);
        toast.success(t("calendar.invite_sent"));
      }
      open = false;
      onsaved();
    } catch (err) {
      app.error(err);
    }
  }

  async function remove() {
    if (!event?.id) return;
    let mode: "one" | "series" = "series";
    if (event.recurrence !== "none" && occurrenceStart != null) {
      mode = (await app.confirm(t("calendar.delete_which"), t("calendar.delete_which_text"), { ok: t("calendar.only_this") })) ? "one" : "series";
      if (mode === "series" && !(await app.confirm(t("calendar.delete_series"), undefined, { ok: t("common.delete"), destructive: true }))) return;
    } else if (!(await app.confirm(t("calendar.delete_confirm", event.title), undefined, { ok: t("common.delete"), destructive: true }))) return;
    if ((e.attendees || e.optional_attendees || e.resources) && app.defaultAccountId() && (await app.confirm(t("calendar.send_cancel"), undefined, { ok: t("common.send") }))) {
      await api.sendInvitation(event.id, app.defaultAccountId()!, true);
    }
    await api.deleteEvent(event.id, mode, occurrenceStart);
    open = false;
    onsaved();
  }

  async function exportIcs() {
    if (!event?.id) return;
    const path = await saveDialog({ defaultPath: `${e.title || "event"}.ics`, filters: [{ name: "iCalendar", extensions: ["ics"] }] });
    if (path) await api.exportEvent(event.id, path);
  }

  const REMINDERS = [-1, 0, 5, 10, 15, 30, 60, 120, 1440, 2880, 10080];
  function reminderLabel(m: number): string {
    if (m < 0) return t("reminder.none");
    if (m === 0) return t("reminder.at_start");
    if (m < 60) return t("reminder.minutes", m);
    if (m < 1440) return t("reminder.hours", m / 60);
    if (m < 10080) return t("reminder.days", m / 1440);
    return t("reminder.weeks", m / 10080);
  }
</script>

{#if !windowed}
<Dialog.Root bind:open>
  {#if readonly && event}
  <!-- Başkasının takvimindeki öğe: salt okunur, paylaşılan izin kadar ayrıntı -->
  <Dialog.Content class="max-h-[92vh] overflow-y-auto sm:max-w-2xl" interactOutsideBehavior="ignore">
    <Dialog.Header>
      <Dialog.Title class="pr-6 text-lg break-words">{event.is_private ? t("calendar.private_item") : event.title}</Dialog.Title>
    </Dialog.Header>
    <div class="bg-muted/60 rounded px-3 py-1.5 text-xs">
      {onlyFreeBusy ? t("calendar.ro_freebusy", roCal?.name ?? "") : event.is_private ? t("calendar.ro_private", roCal?.name ?? "") : t("calendar.ro_notice", roCal?.name ?? "")}
    </div>
    <div class="selectable grid grid-cols-[110px_1fr] gap-x-3 gap-y-2 text-sm">
      <span class="text-muted-foreground">{t("calendar.when")}</span><span>{fmtRange(event)}</span>
      {#if event.location && !event.is_private}<span class="text-muted-foreground">{t("calendar.location")}</span><span class="break-words">{event.location}</span>{/if}
      <span class="text-muted-foreground">{t("calendar.show_as")}</span><span>{t(`showas.${event.show_as || "busy"}`)}</span>
      {#if roCal}<span class="text-muted-foreground">{t("calendar.calendar")}</span><span>{roCal.name}</span>{/if}
      {#if event.organizer && !event.is_private}<span class="text-muted-foreground">{t("calendar.organizer")}</span><span class="break-words">{event.organizer}</span>{/if}
      {#if list(event.attendees).length && !event.is_private}<span class="text-muted-foreground">{t("calendar.attendees")}</span><span class="break-words">{list(event.attendees).join("; ")}</span>{/if}
      {#if list(event.optional_attendees).length && !event.is_private}<span class="text-muted-foreground">{t("calendar.optional")}</span><span class="break-words">{list(event.optional_attendees).join("; ")}</span>{/if}
      {#if list(event.resources).length && !event.is_private}<span class="text-muted-foreground">{t("calendar.rooms")}</span><span class="break-words">{list(event.resources).join("; ")}</span>{/if}
    </div>
    {#if event.description && !event.is_private}
      <div class="selectable max-h-80 overflow-y-auto rounded border p-3 text-sm whitespace-pre-wrap">{event.description}</div>
    {/if}
    <Dialog.Footer>
      <Button data-default-button onclick={() => (open = false)}>{t("common.close")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
  {:else if !windowed}
  <Dialog.Content class="max-h-[92vh] overflow-y-auto sm:max-w-3xl" interactOutsideBehavior="ignore">
    <Dialog.Header>
      <Dialog.Title>{event?.id ? (isMeeting ? t("calendar.meeting") : t("calendar.edit_event")) : isMeeting ? t("calendar.new_meeting") : t("calendar.new_event")}</Dialog.Title>
    </Dialog.Header>
    <div class="grid gap-3">
      {@render editForm()}
      {#if people.length}
        <div class="min-w-0 rounded-md border">
          <button class="hover:bg-accent flex w-full items-center justify-between px-3 py-1.5 text-left text-sm font-medium" onclick={() => (schedOpen = !schedOpen)}>
            {t("calendar.scheduling")}<span class="text-muted-foreground text-xs">{schedOpen ? "▴" : "▾"}</span>
          </button>
          {#if schedOpen}<div class="border-t p-2">{@render scheduler(false)}</div>{/if}
        </div>
      {/if}
      <Textarea rows={6} placeholder={t("calendar.description")} bind:value={e.description} />
    </div>
    <Dialog.Footer class="flex-wrap gap-2 sm:justify-between">
      <div class="flex gap-2">
        {#if event?.id}
          <Button variant="destructive" onclick={remove}>{t("common.delete")}</Button>
          <Button variant="outline" onclick={exportIcs}>{t("calendar.export_event")}</Button>
        {/if}
      </div>
      <div class="flex gap-2">
        <Button variant="outline" onclick={() => (open = false)}>{t("common.cancel")}</Button>
        {#if attendees.includes("@") || optional.includes("@") || rooms.includes("@")}
          <Button variant="secondary" onclick={() => save(true)}>{t("calendar.save_send")}</Button>
        {/if}
        <Button onclick={() => save(false)}>{t("common.save")}</Button>
      </div>
    </Dialog.Footer>
  </Dialog.Content>
  {/if}
</Dialog.Root>
{/if}

{#if windowed && !readonly}
  <!-- Ayrı pencere (Outlook "Toplantı" penceresi): başlık, şerit, sekmeler -->
  <div class="bg-background flex h-screen flex-col overflow-hidden">
    <WindowTitleBar title={windowTitle} onsave={() => save(false)} onclose={closeWindow} />
    <div class="bg-chrome flex shrink-0 items-end gap-1 border-b px-2 text-[13px]">
      <button class="border-b-2 px-3 py-1.5 {tab === 'main' ? 'border-primary text-primary font-semibold' : 'border-transparent hover:bg-black/5'}" onclick={() => (tab = "main")}>{isMeeting ? t("calendar.meeting") : t("calendar.appointment")}</button>
      <button class="border-b-2 px-3 py-1.5 {tab === 'sched' ? 'border-primary text-primary font-semibold' : 'border-transparent hover:bg-black/5'}" onclick={() => (tab = "sched")}>{t("calendar.scheduling")}</button>
    </div>
    <div class="flex shrink-0 flex-wrap items-center gap-1 border-b px-2 py-1.5 text-[13px]">
      {#if isMeeting && (attendees.includes("@") || optional.includes("@") || rooms.includes("@"))}
        <button class="hover:bg-accent flex h-8 items-center gap-1.5 rounded px-3 font-medium" onclick={() => save(true)}><Send class="text-primary size-4" />{t("common.send")}</button>
      {/if}
      <button class="hover:bg-accent flex h-8 items-center gap-1.5 rounded px-3" onclick={() => save(false)}><SaveIcon class="size-4 text-[#7a3e9d]" />{t("calendar.save_close")}</button>
      {#if event?.id}
        <button class="hover:bg-accent flex h-8 items-center gap-1.5 rounded px-3" onclick={remove}><Trash class="size-4" />{t("common.delete")}</button>
        <button class="hover:bg-accent flex h-8 items-center gap-1.5 rounded px-3" onclick={exportIcs}><CalendarDays class="size-4" />{t("calendar.export_event")}</button>
      {/if}
      <span class="bg-border mx-1 h-6 w-px"></span>
      <button class="hover:bg-accent flex h-8 items-center gap-1.5 rounded px-3 {tab === 'sched' ? 'bg-accent' : ''}" onclick={() => (tab = tab === "sched" ? "main" : "sched")}><CalendarClock class="size-4" />{t("calendar.scheduling")}</button>
      <button class="hover:bg-accent flex h-8 items-center gap-1.5 rounded px-3" onclick={() => { tab = "main"; void openRooms(); }}><DoorOpen class="size-4" />{t("calendar.room_finder")}</button>
    </div>
    <div class="min-h-0 flex-1 overflow-auto">
      {#if tab === "main"}
        <div class="flex min-h-full flex-col gap-3 px-4 py-3">
          {@render editForm()}
          <Textarea rows={14} class="min-h-60 flex-1" placeholder={t("calendar.description")} bind:value={e.description} />
        </div>
      {:else}
        <div class="flex h-full min-h-0 flex-col p-3">{@render scheduler(true)}</div>
      {/if}
    </div>
  </div>
{/if}

{#snippet editForm()}
      <Input placeholder={t("calendar.title_placeholder")} bind:value={e.title} class="text-base font-medium" />
      <div class="grid grid-cols-[90px_1fr] items-center gap-2 text-sm">
        <Label>{t("calendar.attendees")}</Label>
        <div class="flex min-w-0 items-start gap-1">
          <!-- Çok katılımcıda taşmaz: yükseklik sınırlı, içi kaydırılır -->
          <div class="border-input flex max-h-28 min-h-9 min-w-0 flex-1 items-start overflow-y-auto overscroll-contain rounded-md border px-2 py-0.5"><AddressInput bind:value={attendees} placeholder={t("calendar.attendees_hint")} /></div>
          {#if !showOptional}<Button variant="ghost" size="sm" class="h-9 shrink-0" onclick={() => (showOptional = true)}>{t("calendar.optional")}</Button>{/if}
        </div>
        {#if showOptional}
          <Label>{t("calendar.optional")}</Label>
          <div class="border-input flex max-h-28 min-h-9 min-w-0 items-start overflow-y-auto overscroll-contain rounded-md border px-2 py-0.5"><AddressInput bind:value={optional} /></div>
        {/if}
        <Label>{t("calendar.rooms")}</Label>
        <div class="relative flex min-w-0 items-start gap-1">
          <div class="border-input flex max-h-20 min-h-9 min-w-0 flex-1 items-start overflow-y-auto overscroll-contain rounded-md border px-2 py-0.5"><AddressInput rooms bind:value={rooms} placeholder={t("calendar.rooms_hint")} /></div>
          <Button variant="outline" size="sm" class="h-9 shrink-0" onclick={openRooms}>{t("calendar.room_finder")}</Button>
          {#if roomOpen}
            <div class="bg-popover text-popover-foreground absolute top-full right-0 z-[95] mt-1 w-80 rounded-md border shadow-lg">
              <div class="border-b p-2">
                <Input placeholder={t("calendar.room_search")} bind:value={roomQuery} class="h-8" />
                <label class="mt-1.5 flex items-center gap-1.5 text-xs"><Checkbox bind:checked={onlyFree} />{t("calendar.only_free_rooms")}</label>
              </div>
              <div class="max-h-72 overflow-y-auto p-1">
                {#if roomsLoading}
                  <div class="text-muted-foreground p-3 text-center text-xs">{t("common.loading")}</div>
                {:else if !roomGroups.length && !dirRooms.length}
                  <div class="text-muted-foreground p-3 text-center text-xs">{t("calendar.no_rooms")}</div>
                {/if}
                {#if !roomGroups.length && dirRooms.length}
                  <div class="text-muted-foreground px-2 pt-2 pb-1 text-[11px] font-semibold uppercase">{t("calendar.rooms_in_directory")}</div>
                  {#each dirRooms as r (r.email)}
                    <button class="hover:bg-accent flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm" onclick={() => addRoom(r)}>
                      <span class="flex min-w-0 flex-1 flex-col"><span class="truncate">{r.name}</span><span class="text-muted-foreground truncate text-xs">{r.email}</span></span>
                    </button>
                  {/each}
                {/if}
                {#each roomGroups as [list, items] (list)}
                  <div class="text-muted-foreground px-2 pt-2 pb-1 text-[11px] font-semibold uppercase">{list}</div>
                  {#each items as r (r.email)}
                    {@const st = roomFb[r.email]}
                    <button class="hover:bg-accent flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm" onclick={() => addRoom(r)}>
                      <span class="flex min-w-0 flex-1 flex-col"><span class="truncate">{r.name}</span><span class="text-muted-foreground truncate text-xs">{r.email}</span></span>
                      {#if st === "free"}<span class="shrink-0 rounded bg-green-600/15 px-1.5 text-[11px] font-medium text-green-700 dark:text-green-400">{t("calendar.room_free")}</span>
                      {:else if st === "busy"}<span class="shrink-0 rounded bg-red-600/15 px-1.5 text-[11px] font-medium text-red-700 dark:text-red-400">{t("showas.busy")}</span>
                      {:else if st === undefined && Object.keys(roomFb).length === 0}<span class="text-muted-foreground shrink-0 text-[11px]">…</span>{/if}
                    </button>
                  {/each}
                {/each}
              </div>
              <div class="border-t p-1 text-right"><Button size="sm" variant="ghost" onclick={() => (roomOpen = false)}>{t("common.close")}</Button></div>
            </div>
          {/if}
        </div>
        <Label>{t("calendar.location")}</Label>
        <Input bind:value={e.location} />
        <Label>{t("calendar.start")}</Label>
        <div class="flex flex-wrap items-center gap-2">
          <DateField bind:value={startDate} onchange={() => { if (endDate != null && startDate != null && endDate < startDate) endDate = startDate; }} />
          {#if !e.all_day}<TimeField bind:value={startTime} />{/if}
          <label class="flex items-center gap-1.5 text-sm"><Checkbox bind:checked={e.all_day} />{t("calendar.all_day")}</label>
        </div>
        <Label>{t("calendar.end")}</Label>
        <div class="flex items-center gap-2">
          <DateField bind:value={endDate} />
          {#if !e.all_day}<TimeField bind:value={endTime} />{/if}
        </div>
        <Label>{t("calendar.repeat")}</Label>
        <div class="flex flex-wrap items-center gap-2">
          <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={e.recurrence}>
            {#each ["none", "daily", "weekdays", "weekly", "monthly", "yearly"] as r (r)}<option value={r}>{t(`repeat.${r}`)}</option>{/each}
          </select>
          {#if e.recurrence !== "none" && e.recurrence !== "weekdays"}
            <span>{t("repeat.every")}</span>
            <Input type="number" min="1" max="99" class="w-16" bind:value={e.recur_interval} />
          {/if}
          {#if e.recurrence !== "none"}
            <span>{t("repeat.until")}</span><DateField bind:value={untilDate} clearable />
          {/if}
        </div>
        <Label>{t("calendar.reminder")}</Label>
        <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={e.reminder_minutes}>
          {#each REMINDERS as r (r)}<option value={r}>{reminderLabel(r)}</option>{/each}
        </select>
        <Label>{t("calendar.show_as")}</Label>
        <div class="flex flex-wrap items-center gap-2">
          <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={e.show_as}>
            {#each ["free", "tentative", "busy", "oof", "elsewhere"] as s (s)}<option value={s}>{t(`showas.${s}`)}</option>{/each}
          </select>
          <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={e.calendar_id} title={t("calendar.calendar")}>
            {#each calendars as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
          </select>
          <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={e.category} title={t("mail.categorize")}>
            <option value="">{t("calendar.no_category")}</option>
            {#each app.categories as c (c.name)}<option value={c.name}>{c.name}</option>{/each}
          </select>
          <label class="flex items-center gap-1.5 text-sm"><Checkbox bind:checked={e.is_private} />{t("calendar.private")}</label>
        </div>
      </div>
{/snippet}

{#snippet scheduler(full: boolean)}
  {@const [ms, me] = meetingSlots()}
  {@const cellW = full ? (workOnly ? 44 : 30) : 0}
  <div class="flex min-h-0 flex-1 flex-col text-xs">
    <div class="mb-2 flex flex-wrap items-center gap-2">
      <Button variant="outline" size="sm" class="h-7 px-2" onclick={() => shiftDay(-1)}>‹</Button>
      <span class="min-w-48 text-center text-[13px] font-semibold">{startDate != null ? new Date(startDate * 1000).toLocaleDateString(i18n.locale, { weekday: "long", day: "numeric", month: "long", year: "numeric" }) : ""}</span>
      <Button variant="outline" size="sm" class="h-7 px-2" onclick={() => shiftDay(1)}>›</Button>
      {#if full}
        <span class="bg-border mx-1 h-5 w-px"></span>
        <Button variant="outline" size="sm" class="h-7 px-2" onclick={() => autoPick(-1)}>« {t("calendar.auto_prev")}</Button>
        <Button variant="outline" size="sm" class="h-7 px-2" onclick={() => autoPick(1)}>{t("calendar.auto_next")} »</Button>
        <label class="ml-2 flex items-center gap-1.5"><Checkbox bind:checked={workOnly} />{t("calendar.work_hours_only")}</label>
        <span class="flex-1"></span>
        <Button variant="outline" size="sm" class="h-7 px-2" onclick={() => loadFreeBusy(true)}>{t("common.refresh")}</Button>
      {:else}
        <span class="text-muted-foreground ml-2">{t("calendar.sched_hint")}</span>
      {/if}
    </div>
    {#if fbError}
      <div class="mb-2 rounded border border-red-300 bg-red-50 px-2 py-1 text-red-800 dark:border-red-900 dark:bg-red-950 dark:text-red-200">{fbError}</div>
    {/if}
    <div class="min-h-0 flex-1 overflow-auto rounded border {full ? 'bg-white dark:bg-transparent' : ''}">
      <div class="grid" style="grid-template-columns: {full ? 260 : 180}px repeat({slotCount}, {full ? `${cellW}px` : 'minmax(18px,1fr)'}); min-width: {full ? 260 + slotCount * cellW : 640}px">
        <!-- Saat başlığı -->
        <div class="bg-background sticky top-0 left-0 z-20 border-b px-2 py-1 font-semibold">{t("calendar.attendees")}</div>
        {#each Array(slotCount) as _, i (i)}
          <div class="bg-background text-muted-foreground sticky top-0 z-10 border-b border-l pl-1 py-1 text-[11px] {i % 2 === 0 ? 'font-medium' : ''}">{i % 2 === 0 ? `${String(WORK_START + i / 2).padStart(2, "0")}:00` : ""}</div>
        {/each}
        <!-- Tüm Katılımcılar -->
        <div class="bg-background sticky left-0 z-10 truncate border-b px-2 py-1 font-semibold">{t("calendar.all_attendees")}</div>
        {#each Array(slotCount) as _, i (i)}
          {@const st = allAt(i)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div role="button" tabindex="-1" class="relative cursor-pointer border-b border-l {full ? 'h-8' : 'h-5'} {FB_CLASS[st] ?? ''}" onclick={() => pickSlot(i)}>
            {#if i >= ms && i < me}<div class="border-primary bg-primary/15 absolute inset-0 border-y-2 {i === Math.floor(ms) ? 'border-l-2 border-l-green-600' : ''} {i === Math.ceil(me) - 1 ? 'border-r-2 border-r-red-600' : ''}"></div>{/if}
          </div>
        {/each}
        {#each people as p (p.addr + p.role)}
          {@const PIcon = p.role === "room" ? DoorOpen : UserIcon}
          <div class="bg-background sticky left-0 z-10 flex min-w-0 items-center gap-1.5 border-b px-2 {full ? 'h-9' : 'py-0.5'}" title={p.addr}>
            <PIcon class="text-muted-foreground size-3.5 shrink-0" />
            <span class="min-w-0 flex-1 truncate">{p.name || p.addr}</span>
            {#if full}<span class="text-muted-foreground shrink-0 text-[10px]">{p.role === "req" ? t("calendar.required") : p.role === "opt" ? t("calendar.optional") : t("calendar.room")}</span>{/if}
          </div>
          {#if full}
            <!-- Ayrıntılı satır: dilimler + üzerinde konu/konumlu meşgul blokları -->
            <div class="relative border-b" style="grid-column: span {slotCount}">
              <div class="absolute inset-0 grid" style="grid-template-columns: repeat({slotCount}, 1fr)">
                {#each Array(slotCount) as _, i (i)}
                  {@const st = stateAt(p.addr, i)}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <div role="button" tabindex="-1" class="relative h-9 cursor-pointer border-l hover:outline hover:outline-1 hover:outline-primary/60 {st === 4 ? FB_CLASS[4] : ''}" onclick={() => pickSlot(i)}>
                    {#if i >= ms && i < me}<div class="border-primary bg-primary/10 pointer-events-none absolute inset-0 border-y-2 {i === Math.floor(ms) ? 'border-l-2 border-l-green-600' : ''} {i === Math.ceil(me) - 1 ? 'border-r-2 border-r-red-600' : ''}"></div>{/if}
                  </div>
                {/each}
              </div>
              {#each fbEvents[p.addr] ?? [] as ev, k (k)}
                {@const pos = blockPos(ev)}
                {#if pos && ev.show_as !== "free"}
                  <div
                    class="pointer-events-none absolute top-1 bottom-1 overflow-hidden rounded-[2px] px-1 text-[10px] leading-[14px] text-white {ev.show_as === 'tentative' ? 'bg-sky-400' : ev.show_as === 'oof' ? 'bg-purple-600' : ev.show_as === 'elsewhere' ? 'bg-amber-500' : 'bg-blue-600'}"
                    style="left: {pos.left}%; width: {pos.width}%"
                    title="{ev.title || t(`showas.${ev.show_as}`)}{ev.location ? ` — ${ev.location}` : ''}"
                  >
                    <div class="truncate font-medium">{ev.title || t(`showas.${ev.show_as}`)}</div>
                    {#if ev.location}<div class="truncate opacity-90">{ev.location}</div>{/if}
                  </div>
                {/if}
              {/each}
            </div>
          {:else}
            {#each Array(slotCount) as _, i (i)}
              {@const st = stateAt(p.addr, i)}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div role="button" tabindex="-1" class="relative h-5 cursor-pointer border-b border-l hover:outline hover:outline-1 hover:outline-primary/60 {FB_CLASS[st] ?? ''}" onclick={() => pickSlot(i)}>
                {#if i >= ms && i < me}<div class="border-primary bg-primary/15 absolute inset-0 border-y-2"></div>{/if}
              </div>
            {/each}
          {/if}
        {/each}
      </div>
      {#if !people.length}
        <div class="text-muted-foreground p-6 text-center text-sm">{t("calendar.sched_empty")}</div>
      {/if}
    </div>
    <div class="text-muted-foreground mt-2 flex flex-wrap items-center gap-3">
      <span class="flex items-center gap-1"><span class="size-3 bg-blue-600"></span>{t("showas.busy")}</span>
      <span class="flex items-center gap-1"><span class="size-3 bg-sky-300"></span>{t("showas.tentative")}</span>
      <span class="flex items-center gap-1"><span class="size-3 bg-purple-600"></span>{t("showas.oof")}</span>
      <span class="flex items-center gap-1"><span class="size-3 bg-amber-500"></span>{t("showas.elsewhere")}</span>
      <span class="flex items-center gap-1"><span class="size-3 border {FB_CLASS[4]}"></span>{t("calendar.no_info")}</span>
      {#if fbLoading}<span>{t("common.loading")}</span>{/if}
      {#if full}
        <span class="flex-1"></span>
        <span class="text-foreground flex items-center gap-2">
          {t("calendar.start")} <DateField bind:value={startDate} /> <TimeField bind:value={startTime} />
          {t("calendar.end")} <DateField bind:value={endDate} /> <TimeField bind:value={endTime} />
        </span>
      {/if}
    </div>
  </div>
{/snippet}

