<script lang="ts">
  import Ribbon from "$lib/components/ribbon/Ribbon.svelte";
  import FileMenu from "$lib/components/ribbon/FileMenu.svelte";
  import Info from "@lucide/svelte/icons/info";
  // Takvim modülü: Gün / İş haftası / Hafta / Ay / Gündem görünümleri.
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import * as ContextMenu from "$lib/components/ui/context-menu";
  import Splitter from "$lib/components/common/Splitter.svelte";
  import { Button } from "$lib/components/ui/button";
  import RibbonButton from "$lib/components/common/RibbonButton.svelte";
  import MiniMonth from "./MiniMonth.svelte";
  import EventDialog from "./EventDialog.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { i18n, t } from "$lib/i18n.svelte";
  import { addDays, addMonths, fmtDate, fmtDateLong, fmtTime, sameDay, startOfDay, startOfWeek, toDate, toTs, weekdayLong, weekdayShort } from "$lib/format";
  import type { CalEvent, Calendar, Occurrence } from "$lib/types";
  import CalendarPlus from "@lucide/svelte/icons/calendar-plus";
  import CalendarCheck from "@lucide/svelte/icons/calendar-check";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Upload from "@lucide/svelte/icons/upload";
  import Download from "@lucide/svelte/icons/download";
  import Printer from "@lucide/svelte/icons/printer";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Bell from "@lucide/svelte/icons/bell";
  import Users from "@lucide/svelte/icons/users";
  import Lock from "@lucide/svelte/icons/lock";
  import * as Dialog from "$lib/components/ui/dialog";
  import AddressBookPicker from "$lib/components/common/AddressBookPicker.svelte";
  import DoorOpen from "@lucide/svelte/icons/door-open";
  import Globe from "@lucide/svelte/icons/globe";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Plus from "@lucide/svelte/icons/plus";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import CalendarRange from "@lucide/svelte/icons/calendar-range";
  import Calendar1 from "@lucide/svelte/icons/calendar-1";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import CalendarFold from "@lucide/svelte/icons/calendar-fold";
  import ChartGantt from "@lucide/svelte/icons/chart-gantt";
  import Columns3 from "@lucide/svelte/icons/columns-3";
  import Grid3x3 from "@lucide/svelte/icons/grid-3x3";
  import ListIcon from "@lucide/svelte/icons/list";
  import MailPlus from "@lucide/svelte/icons/mail-plus";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import BookUser from "@lucide/svelte/icons/book-user";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import User from "@lucide/svelte/icons/user";
  import Clock from "@lucide/svelte/icons/clock";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import RibbonGroup from "$lib/components/ribbon/RibbonGroup.svelte";
  import RibbonStack from "$lib/components/ribbon/RibbonStack.svelte";
  import { people } from "$lib/stores/people.svelte";
  import { displayName, splitAddresses } from "$lib/format";
  import { open as openDialog, save as saveDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";
  import { untrack } from "svelte";

  type View = "day" | "workweek" | "week" | "month" | "agenda" | "next7" | "schedule";
  let sideW = $state(Number(app.setting("pane_cal_w", "230")) || 230);
  let view = $state<View>((app.setting("calendar_view", "week") as View) || "week");
  let current = $state(startOfDay(new Date()));
  let occurrences = $state<Occurrence[]>([]);
  let calendars = $state<Calendar[]>([]);
  let dialogOpen = $state(false);
  let editing = $state<CalEvent | null>(null);
  let editingOcc = $state<number | null>(null);
  let searchResults = $state<CalEvent[] | null>(null);

  const HOUR = $derived(Number(app.setting("hour_height", "48")));
  const workStart = $derived(Number(app.setting("work_start", "8")));
  const workEnd = $derived(Number(app.setting("work_end", "17")));

  const range = $derived.by(() => {
    if (view === "day" || view === "schedule") return { start: current, days: 1 };
    if (view === "next7") return { start: current, days: 7 };
    if (view === "workweek") return { start: startOfWeek(current), days: 5 };
    if (view === "week") return { start: startOfWeek(current), days: 7 };
    if (view === "agenda") return { start: current, days: 30 };
    const first = startOfWeek(new Date(current.getFullYear(), current.getMonth(), 1));
    return { start: first, days: 42 };
  });

  const days = $derived(Array.from({ length: range.days }, (_, i) => addDays(range.start, i)));

  let loadSeq = 0;
  async function load() {
    const my = ++loadSeq;
    calendars = await api.listCalendars();
    const s = toTs(range.start) - 86400;
    const e = toTs(range.start) + range.days * 86400 + 86400;
    const local = await api.listOccurrences(s, e);
    if (my !== loadSeq) return;
    occurrences = local;
    // Başkalarının (ekip, oda, paylaşılan) görünür takvimleri sunucudan okunur
    const remote = calendars.filter((c) => c.kind === "remote" && c.visible);
    const parts = await Promise.all(
      remote.map((c) =>
        api.remoteOccurrences(c.id, s, e).catch((err) => {
          remoteErrors = { ...remoteErrors, [c.id]: String(err) };
          return [] as Occurrence[];
        }),
      ),
    );
    if (my !== loadSeq) return;
    if (parts.some((p) => p.length)) occurrences = [...local, ...parts.flat()].sort((a, b) => a.start - b.start);
  }
  let remoteErrors = $state<Record<number, string>>({});

  // Outlook "Ekip" grubu: yönetici ve ekip arkadaşları dizinden otomatik (oturum başına bir kez)
  let teamLabel = $state(app.setting("team_group_name", ""));
  let teamTried = false;
  // İnternet takvimleri (en çok saatte bir) yenilenir
  void api.refreshInternetCalendars().then((n) => { if (n) void load(); }).catch(() => {});
  $effect(() => {
    if (teamTried || !app.accounts.some((a) => a.kind === "exchange")) return;
    teamTried = true;
    void api.refreshTeamCalendars().then((label) => {
      teamLabel = label;
      void load();
    }).catch(() => {});
  });

  // ------------------------------------------------------------------ takvim grupları (Outlook)
  // Yerleşik gruplar: "" Takvimlerim, "team" Ekip, "other" Diğer, "shared" Paylaşılan;
  // kullanıcı grupları "g:<kimlik>". Grup ve takvim sırası ayarlarda saklanır.
  type Group = { id: string; name: string; custom: boolean };
  function readJson<T>(key: string, def: T): T {
    try {
      return JSON.parse(app.setting(key, "")) as T;
    } catch {
      return def;
    }
  }
  let customGroups = $state<{ id: string; name: string }[]>(readJson("calendar_groups", []));
  let groupOrder = $state<string[]>(readJson("calendar_group_order", []));
  let calOrder = $state<number[]>(readJson("calendar_order", []));
  const allGroups = $derived.by<Group[]>(() => {
    const list: Group[] = [
      { id: "", name: t("calendar.my_calendars"), custom: false },
      { id: "team", name: teamLabel || t("calendar.my_team"), custom: false },
      { id: "other", name: t("calendar.other_calendars"), custom: false },
      { id: "shared", name: t("calendar.shared_calendars"), custom: false },
      ...customGroups.map((g) => ({ ...g, custom: true })),
    ];
    const idx = (id: string) => {
      const i = groupOrder.indexOf(id);
      return i < 0 ? 1000 + list.findIndex((g) => g.id === id) : i;
    };
    return list.sort((x, y) => idx(x.id) - idx(y.id));
  });
  function groupCalendars(g: string): Calendar[] {
    const list = calendars.filter((c) => (c.grp ?? "") === g || (g === "" && c.grp && !allGroups.some((x) => x.id === c.grp)));
    const idx = (id: number) => {
      const i = calOrder.indexOf(id);
      return i < 0 ? 100000 + id : i;
    };
    return list.sort((a, b) => idx(a.id) - idx(b.id));
  }
  function saveGroups() {
    void app.setSetting("calendar_groups", JSON.stringify(customGroups));
    void app.setSetting("calendar_group_order", JSON.stringify(groupOrder));
  }
  let groupOpen = $state<Record<string, boolean>>({ "": true, team: false, other: true, shared: true });

  /** Outlook: hesabın takvimi "Takvim - adres" biçiminde */
  function calName(c: Calendar): string {
    return c.account_id && !c.kind ? t("calendar.calendar") : c.name;
  }
  async function setGroupVisible(g: string, v: boolean) {
    for (const c of groupCalendars(g)) if (c.visible !== v) await api.saveCalendar({ ...c, visible: v });
    await load();
  }
  async function toggleCalendar(c: Calendar, v = !c.visible) {
    await api.saveCalendar({ ...c, visible: v });
    await load();
  }
  async function newGroup() {
    const name = await app.prompt(t("calendar.new_group"), t("calendar.group_name"));
    if (!name?.trim()) return;
    const id = `g:${Date.now().toString(36)}`;
    customGroups = [...customGroups, { id, name: name.trim() }];
    groupOrder = [...allGroups.map((g) => g.id).filter((x) => x !== id), id];
    groupOpen = { ...groupOpen, [id]: true };
    saveGroups();
  }
  async function renameGroup(g: Group) {
    const name = await app.prompt(t("calendar.rename_group"), t("calendar.group_name"), g.name);
    if (!name?.trim()) return;
    customGroups = customGroups.map((x) => (x.id === g.id ? { ...x, name: name.trim() } : x));
    saveGroups();
  }
  async function removeAll(g: string) {
    for (const c of groupCalendars(g)) {
      if (c.kind === "remote" || c.kind === "internet") await api.deleteCalendar(c.id).catch(() => {});
      else if (g) await api.saveCalendar({ ...c, grp: "" });
    }
    await load();
  }
  async function deleteGroup(g: Group) {
    if (!(await app.confirm(t("calendar.delete_group_confirm", g.name), t("calendar.delete_group_text"), { ok: t("common.delete"), destructive: true }))) return;
    await removeAll(g.id);
    customGroups = customGroups.filter((x) => x.id !== g.id);
    groupOrder = groupOrder.filter((x) => x !== g.id);
    saveGroups();
  }
  function moveGroup(g: string, dir: number) {
    const ids = allGroups.map((x) => x.id);
    const i = ids.indexOf(g);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j], ids[i]];
    groupOrder = ids;
    saveGroups();
  }
  function setCalOrder(order: number[]) {
    calOrder = order;
    void app.setSetting("calendar_order", JSON.stringify(order));
  }
  function sortGroupByName(g: string) {
    const sorted = groupCalendars(g).sort((a, b) => calName(a).localeCompare(calName(b), i18n.locale));
    const rest = calOrder.filter((id) => !sorted.some((c) => c.id === id));
    setCalOrder([...rest, ...sorted.map((c) => c.id)]);
  }
  function moveCalendar(c: Calendar, dir: number) {
    const list = groupCalendars(c.grp ?? "").map((x) => x.id);
    const i = list.indexOf(c.id);
    const j = i + dir;
    if (j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    const rest = calOrder.filter((id) => !list.includes(id));
    setCalOrder([...rest, ...list]);
  }
  async function moveToGroup(c: Calendar, g: string) {
    await api.saveCalendar({ ...c, grp: g });
    await load();
  }
  // Outlook takvim renkleri
  const PALETTE = ["#0f6cbd", "#3a96dd", "#498205", "#5c9e31", "#8764b8", "#c239b3", "#d13438", "#ca5010", "#d29200", "#986f0b", "#038387", "#00b7c3", "#69797e", "#7a7574"];
  async function setColor(c: Calendar, color: string) {
    await api.saveCalendar({ ...c, color });
    await load();
  }

  // Takvim Ekle: Adres Defterinden / Oda Listesinden (Ad Seç penceresi) / İnternet'ten / Paylaşılan
  const MAX_CALENDARS = 30;
  /** Yeni etkinliğin takvimi: varsayılan hesabın Exchange takvimi (Outlook gibi; toplantı
   *  istekleri ve oda rezervasyonu Exchange üzerinden gider), yoksa ilk kendi takvimi */
  function defaultCalendarId(): number {
    const own = calendars.filter((c) => (!c.kind || c.kind === "dav") && !c.owner);
    const acc = app.defaultAccountId();
    return (own.find((c) => c.account_id === acc) ?? own.find((c) => c.account_id) ?? own[0] ?? calendars[0])?.id ?? 1;
  }
  let picker = $state<{ open: boolean; grp: string; source: "gal" | "contacts" | "rooms" }>({ open: false, grp: "other", source: "gal" });
  function openPicker(grp: string, source: "gal" | "contacts" | "rooms" = "gal") {
    picker = { open: true, grp, source };
  }
  async function addRemoteCalendars(entries: string[]) {
    const colors = PALETTE.slice(1);
    let n = 0;
    for (const en of entries.slice(0, MAX_CALENDARS)) {
      const addr = (en.match(/<([^>]+)>/)?.[1] ?? en).trim();
      if (!addr.includes("@") || calendars.some((c) => c.owner?.toLowerCase() === addr.toLowerCase())) continue;
      const name = displayName(en);
      await api.saveCalendar({ id: 0, name: name && !name.includes("@") ? name : people.name(addr) || addr, color: colors[(calendars.length + n) % colors.length], visible: true, owner: addr, grp: picker.grp, kind: "remote" });
      n++;
    }
    groupOpen = { ...groupOpen, [picker.grp]: true };
    if (n) await load();
  }
  async function addInternet(grp: string) {
    const url = await app.prompt(t("calendar.from_internet").replace("...", ""), t("calendar.internet_prompt"), "https://");
    if (!url || url === "https://") return;
    try {
      const n = await api.addInternetCalendar(url, "", grp);
      toast.success(t("calendar.internet_added", n));
      groupOpen = { ...groupOpen, [grp]: true };
      await load();
    } catch (e) {
      app.error(e);
    }
  }


  $effect(() => {
    void app.revision.calendar;
    void range.start.getTime();
    void range.days;
    void load();
  });

  $effect(() => {
    const q = app.search.trim();
    if (!q) {
      searchResults = null;
      return;
    }
    api.searchEvents(q).then((r) => (searchResults = r));
  });

  // Başka modüllerden gelen yeni öğe istekleri
  $effect(() => {
    const r = app.newItemRequest;
    if (r && r.module === "calendar") {
      app.newItemRequest = null;
      newEvent(undefined, r.prefill as Partial<CalEvent>);
    }
  });
  $effect(() => {
    const d = app.calendarGoto;
    if (d != null) {
      app.calendarGoto = null;
      current = startOfDay(new Date(d * 1000));
      if (view === "month" || view === "agenda") setView("day");
    }
  });
  $effect(() => {
    const r = app.openItemRequest;
    if (r && r.module === "calendar") {
      app.openItemRequest = null;
      api.getEvent(r.id).then((e) => e && openEvent(e, null));
    }
  });

  function calColor(id: number): string {
    return calendars.find((c) => c.id === id)?.color ?? "#3a7bd5";
  }

  function evColor(e: CalEvent): string {
    return e.category ? app.categoryColor(e.category) : calColor(e.calendar_id);
  }

  function newEvent(start?: number, prefill: Partial<CalEvent> = {}) {
    const s = start ?? (() => {
      const d = new Date();
      d.setMinutes(d.getMinutes() < 30 ? 30 : 60, 0, 0);
      const base = new Date(current);
      base.setHours(d.getHours(), d.getMinutes());
      return toTs(base);
    })();
    const ev: CalEvent = { id: 0, calendar_id: defaultCalendarId(), uid: "", title: "", location: "", description: "", start: s, end: s + Number(app.setting("default_duration", "30")) * 60, all_day: false, recurrence: "none", recur_interval: 1, recur_until: null, exdates: "", reminder_minutes: Number(app.setting("default_reminder", "15")), category: "", show_as: "busy", attendees: "", is_private: false, ...prefill };
    // Outlook gibi ayrı pencerede açılır ("Yeni Toplantı" katılımcı alanıyla gelir)
    const meeting = "attendees" in prefill;
    void api.openWindow("event", { event: $state.snapshot(ev), occurrenceStart: null, meeting }, meeting ? t("calendar.new_meeting") : t("calendar.new_event")).catch((e) => app.error(e));
  }

  /** Başkasının takvimi, internet takvimi, doğum günü: düzenlenemez */
  let editingReadonly = $state(false);
  function openEvent(e: CalEvent, occStart: number | null) {
    // Salt okunur öğeler de çift tıklamayla açılır (paylaşılan izin kadar ayrıntıyla)
    editingReadonly = e.id < 0 || (!!e.master_id && !e.remote_id) || calendars.find((c) => c.id === e.calendar_id)?.kind === "internet";
    tip = null;
    if (!editingReadonly) {
      const meeting = !!(e.attendees || e.optional_attendees || e.resources);
      void api
        .openWindow("event", { event: $state.snapshot(e), occurrenceStart: occStart, meeting }, e.title || (meeting ? t("calendar.meeting") : t("calendar.appointment")))
        .catch((err) => app.error(err));
      return;
    }
    editing = e;
    editingOcc = occStart;
    dialogOpen = true;
  }

  function move(dir: number) {
    if (view === "month") current = addMonths(current, dir);
    else if (view === "day" || view === "schedule") current = addDays(current, dir);
    else if (view === "agenda") current = addDays(current, dir * 30);
    else current = addDays(current, dir * 7);
  }

  function setView(v: View) {
    view = v;
    void app.setSetting("calendar_view", v);
  }

  // Outlook: "12 - 18 Ekim 2026", ay farklıysa "28 Eylül - 4 Ekim 2026"
  const title = $derived.by(() => {
    if (view === "day" || view === "schedule") return fmtDateLong(current);
    if (view === "month") return current.toLocaleDateString(i18n.locale, { month: "long", year: "numeric" });
    const s = range.start;
    const end = addDays(range.start, range.days - 1);
    const month = (d: Date) => d.toLocaleDateString(i18n.locale, { month: "long" });
    if (s.getMonth() === end.getMonth()) return `${s.getDate()} - ${end.getDate()} ${month(end)} ${end.getFullYear()}`;
    if (s.getFullYear() === end.getFullYear()) return `${s.getDate()} ${month(s)} - ${end.getDate()} ${month(end)} ${end.getFullYear()}`;
    return `${fmtDate(s, { day: "numeric", month: "long", year: "numeric" })} - ${fmtDate(end, { day: "numeric", month: "long", year: "numeric" })}`;
  });

  const VIEWS: [View, string, typeof CalendarIcon][] = [
    ["day", "calendar.view_day", Calendar1],
    ["workweek", "calendar.view_workweek", Columns3],
    ["week", "calendar.view_week", CalendarDays],
    ["month", "calendar.view_month", Grid3x3],
    ["schedule", "calendar.view_schedule", ChartGantt],
    ["agenda", "calendar.view_agenda", ListIcon],
  ];

  // Sol bölmedeki iki ay (Outlook Tarih Gezgini)
  let navMonth = $state(new Date(new Date().getFullYear(), new Date().getMonth(), 1));
  $effect(() => {
    const c = current;
    untrack(() => {
      const nm = navMonth;
      const inShown = (c.getFullYear() === nm.getFullYear() && c.getMonth() === nm.getMonth()) || (addMonths(nm, 1).getFullYear() === c.getFullYear() && addMonths(nm, 1).getMonth() === c.getMonth());
      if (!inShown) navMonth = new Date(c.getFullYear(), c.getMonth(), 1);
    });
  });

  // ------------------------------------------------------------------ etkinlik görünümü
  type Look = { fill: string; bar: string; border: string; dashed: boolean; text: string };
  /** Outlook: meşgulde dolu şerit, belirsiz/yanıtlanmamışta çizgili şerit ve kesikli kenar,
   *  boşta beyaz şerit, ofis dışında mor; iptal edilende beyaz kutu */
  function look(e: CalEvent): Look {
    const c = evColor(e);
    if (e.is_cancelled) return { fill: "var(--background)", bar: "var(--background)", border: "color-mix(in srgb, " + c + " 60%, #888)", dashed: false, text: "inherit" };
    const fill = `color-mix(in srgb, ${c} 26%, var(--background))`;
    const tentative = e.show_as === "tentative" || e.response === "none" || e.response === "tentative";
    let bar = c;
    if (tentative) bar = `repeating-linear-gradient(135deg, ${c} 0 3px, var(--background) 3px 6px)`;
    else if (e.show_as === "free") bar = "var(--background)";
    else if (e.show_as === "oof") bar = "#7b2fa1";
    else if (e.show_as === "elsewhere") bar = `repeating-linear-gradient(0deg, ${c} 0 2px, var(--background) 2px 4px)`;
    return { fill, bar, border: c, dashed: tentative, text: "inherit" };
  }

  // Seçili etkinlik (tek tık) ve üzerine gelince ayrıntı kartı
  let selectedOcc = $state<string | null>(null);
  let tip = $state<{ o: Occurrence; x: number; y: number } | null>(null);
  let tipTimer: ReturnType<typeof setTimeout> | undefined;
  let lastMouse = { x: 0, y: 0 };
  function tipEnter(e: MouseEvent, o: Occurrence) {
    clearTimeout(tipTimer);
    lastMouse = { x: e.clientX, y: e.clientY };
    tipTimer = setTimeout(() => {
      if (!drag) tip = { o, x: lastMouse.x, y: lastMouse.y };
    }, 450);
  }
  function tipMove(e: MouseEvent) {
    // Bekleme süresince fare konumu izlenir (kart imlecin yanında açılır)
    if (!tip) lastMouse = { x: e.clientX, y: e.clientY };
  }
  function tipLeave() {
    clearTimeout(tipTimer);
    tip = null;
  }
  const RESP: Record<string, string> = { organizer: "calendar.resp_organizer", accept: "calendar.resp_accept", tentative: "calendar.resp_tentative", decline: "calendar.resp_decline", none: "calendar.resp_none" };

  function occKey(o: Occurrence) {
    return o.event.id + "-" + o.start;
  }

  // ------------------------------------------------------------------ Takvimi E-postayla Gönder
  async function emailCalendar() {
    try {
      const s = toTs(range.start);
      const f = await api.calendarIcsTemp(s, s + range.days * 86400, `${t("calendar.calendar")} ${title}`);
      app.compose({ mode: "new", data: { subject: `${t("calendar.calendar")}: ${title}` }, attachments: [{ name: f.name, mime: f.mime, size: f.size, path: f.path }] });
    } catch (e) {
      app.error(e);
    }
  }

  // Kişi Ara (Outlook "Bul" grubu): ad yazılır, Enter ile kişi kartı açılır
  let personQuery = $state("");
  async function findPerson(ev: KeyboardEvent) {
    if (ev.key !== "Enter" || !personQuery.trim()) return;
    const el = ev.currentTarget as HTMLElement;
    const r = await api.suggestRecipients(personQuery.trim(), true).catch(() => []);
    if (!r[0]) {
      toast.error(t("common.no_results"));
      return;
    }
    const rect = el.getBoundingClientRect();
    people.card = { email: r[0].email, name: r[0].name, x: rect.left, y: rect.bottom + 4 };
  }

  // Zamanlama Görünümü: saat başına genişlik
  const SCHED_W = 72;
  let schedEl: HTMLDivElement | undefined = $state();
  $effect(() => {
    if (schedEl && view === "schedule") schedEl.scrollLeft = Math.max(0, (workStart - 1) * SCHED_W);
  });

  // Tarih Gezgini'nde etkinlikli günler kalın: gösterilen iki ayın etkinlikleri ayrıca alınır
  let busyDays = $state(new Set<string>());
  $effect(() => {
    void app.revision.calendar;
    const s = toTs(startOfWeek(navMonth));
    void api.listOccurrences(s, s + 75 * 86400).then((list) => {
      busyDays = new Set(list.map((o) => { const d = toDate(o.start); return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`; }));
    }).catch(() => {});
  });

  // ---------------------------------------------------------------- zaman ızgarası yerleşimi
  function dayOccurrences(d: Date, allDay: boolean): Occurrence[] {
    const s = toTs(d);
    const e = s + 86400;
    return occurrences.filter((o) => {
      const isAll = o.event.all_day || o.end - o.start >= 86400;
      return isAll === allDay && o.start < e && o.end > s;
    });
  }

  function layout(list: Occurrence[], dayStart: number) {
    // Çakışan etkinlikleri yan yana sütunlara yerleştir
    const sorted = [...list].sort((a, b) => a.start - b.start || b.end - a.end);
    const cols: number[] = [];
    const placed: { o: Occurrence; col: number; top: number; height: number; group: number }[] = [];
    let groupEnd = 0;
    let group = 0;
    const groupCols = new Map<number, number>();
    for (const o of sorted) {
      if (o.start >= groupEnd) {
        group++;
        cols.length = 0;
      }
      let c = cols.findIndex((end) => end <= o.start);
      if (c < 0) {
        c = cols.length;
        cols.push(o.end);
      } else cols[c] = o.end;
      groupEnd = Math.max(groupEnd, o.end);
      groupCols.set(group, Math.max(groupCols.get(group) ?? 0, cols.length));
      const top = Math.max(0, (o.start - dayStart) / 3600) * HOUR;
      const bottom = Math.min(24, (o.end - dayStart) / 3600) * HOUR;
      placed.push({ o, col: c, top, height: Math.max(18, bottom - top), group });
    }
    return placed.map((p) => ({ ...p, cols: groupCols.get(p.group) ?? 1 }));
  }

  // ---------------------------------------------------------------- sürükleme
  let drag = $state<{ o: Occurrence; mode: "move" | "resize"; y0: number; x0: number; dy: number; dayShift: number; colWidth: number } | null>(null);

  function startDrag(ev: PointerEvent, o: Occurrence, mode: "move" | "resize", colWidth: number) {
    if (ev.button !== 0) return;
    ev.stopPropagation();
    if (o.event.id < 0) return;
    drag = { o, mode, y0: ev.clientY, x0: ev.clientX, dy: 0, dayShift: 0, colWidth };
  }

  function onmove(ev: PointerEvent) {
    if (!drag) return;
    drag.dy = ev.clientY - drag.y0;
    drag.dayShift = drag.mode === "move" ? Math.round((ev.clientX - drag.x0) / drag.colWidth) : 0;
  }

  async function onup() {
    if (!drag) return;
    const d = drag;
    drag = null;
    const minutes = Math.round(((d.dy / HOUR) * 60) / 15) * 15;
    if (Math.abs(d.dy) < 4 && d.dayShift === 0) return;
    const delta = minutes * 60 + d.dayShift * 86400;
    let ev = d.o.event;
    try {
      if (ev.recurrence !== "none") {
        const id = await api.detachOccurrence(ev.id, d.o.start);
        const fresh = await api.getEvent(id);
        if (!fresh) return;
        ev = fresh;
      }
      const upd = { ...ev };
      if (d.mode === "move") {
        upd.start += delta;
        upd.end += delta;
      } else {
        upd.end = Math.max(upd.start + 900, upd.end + minutes * 60);
      }
      await api.saveEvent(upd);
      await load();
    } catch (e) {
      app.error(e);
    }
  }

  function slotClick(ev: MouseEvent, day: Date) {
    const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    const y = ev.clientY - rect.top;
    const minutes = Math.floor(((y / HOUR) * 60) / 30) * 30;
    newEvent(toTs(day) + minutes * 60);
  }

  async function importIcs() {
    const p = await openDialog({ filters: [{ name: "iCalendar", extensions: ["ics", "ical", "vcs"] }] });
    if (p && typeof p === "string") {
      toast.success(t("import.events_done", await api.importIcs(p)));
      await load();
    }
  }

  async function exportIcs() {
    const p = await saveDialog({ defaultPath: "takvim.ics", filters: [{ name: "iCalendar", extensions: ["ics"] }] });
    if (p) toast.success(t("export.done", await api.exportIcs(p)));
  }

  async function addCalendar() {
    const name = await app.prompt(t("calendar.new_calendar"), t("calendar.calendar_name"));
    if (!name) return;
    const colors = ["#3a7bd5", "#27ae60", "#e67e22", "#8e44ad", "#e74c3c", "#16a085", "#d35400"];
    await api.saveCalendar({ id: 0, name, color: colors[calendars.length % colors.length], visible: true });
    await load();
  }

  async function renameCalendar(c: Calendar) {
    const name = await app.prompt(t("calendar.rename_calendar"), t("calendar.calendar_name"), c.name);
    if (name) {
      await api.saveCalendar({ ...c, name });
      await load();
    }
  }

  async function deleteCalendar(c: Calendar) {
    if (await app.confirm(t("calendar.delete_calendar", c.name), t("calendar.delete_calendar_text"), { ok: t("common.delete"), destructive: true })) {
      try {
        await api.deleteCalendar(c.id);
        await load();
      } catch (e) {
        app.error(e);
      }
    }
  }

  function printAgenda() {
    const w = window.open("", "_blank");
    if (!w) return;
    const rows = occurrences
      .filter((o) => o.start >= toTs(range.start) && o.start < toTs(range.start) + range.days * 86400)
      .map((o) => `<tr><td>${fmtDate(o.start, { weekday: "short", day: "numeric", month: "short" })}</td><td>${o.event.all_day ? t("calendar.all_day") : fmtTime(o.start) + " – " + fmtTime(o.end)}</td><td><b>${o.event.title}</b><br>${o.event.location}</td></tr>`)
      .join("");
    w.document.write(`<html><head><meta charset="utf-8"><title>${title}</title><style>body{font-family:sans-serif}td{border-bottom:1px solid #ccc;padding:6px;vertical-align:top}</style></head><body><h2>${title}</h2><table>${rows}</table></body></html>`);
    w.document.close();
    w.print();
  }

  let gridEl: HTMLDivElement | undefined = $state();
  $effect(() => {
    if (gridEl && (view === "day" || view === "week" || view === "workweek" || view === "next7")) gridEl.scrollTop = Math.max(0, (workStart - 0.5) * HOUR);
  });

  const now = $state({ ts: Date.now() / 1000 });
  setInterval(() => (now.ts = Date.now() / 1000), 60000);
</script>

<svelte:window onpointermove={onmove} onpointerup={onup} />

{#snippet eventBox(o: Occurrence, compact: boolean)}
  {@const ev = o.event}
  {@const lk = look(ev)}
  {@const org = ev.organizer ? displayName(ev.organizer) : ""}
  <div class="pointer-events-none absolute inset-y-0 left-0 w-[6px]" style="background:{lk.bar};border-right:1px solid {lk.border}"></div>
  <div class="pl-2 leading-[13px]">
    <div class="flex items-start gap-1">
      {#if ev.is_private}<Lock class="mt-px size-3 shrink-0" />{/if}
      <span class="font-semibold break-words {compact ? 'truncate' : ''}">{ev.title || t("calendar.untitled")}</span>
    </div>
    {#if !compact}
      {#if ev.location}<div class="break-words">{ev.location}</div>{/if}
      {#if org}<div class="break-words">{org}</div>{/if}
    {/if}
  </div>
  {#if ev.recurrence !== "none" || ev.master_id}
    <Repeat class="absolute right-0.5 bottom-0.5 size-3 opacity-70" />
  {/if}
{/snippet}

<div class="flex h-full min-h-0 flex-col">
  <Ribbon tabs={[{ key: "home", label: t("ribbon.home") }, { key: "view", label: t("ribbon.view") }, { key: "help", label: t("ribbon.help") }]}>
    {#snippet file()}<FileMenu />{/snippet}
    {#snippet content(tab)}
      {#if tab === "help"}
        <RibbonGroup last><RibbonButton icon={Info} label={t("settings.tab_about")} onclick={() => app.openSettings("about")} /></RibbonGroup>
      {:else if tab === "view"}
        <RibbonGroup label={t("ribbon.g_arrange")}>
          {#each VIEWS as [v, label, Icon] (v)}
            <RibbonButton icon={Icon} label={t(label)} active={view === v} onclick={() => setView(v)} />
          {/each}
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_time_scale")} last>
          <RibbonStack>
            {#each [["32", "calendar.scale_60"], ["48", "calendar.scale_30"], ["72", "calendar.scale_15"]] as [h, label] (h)}
              <RibbonButton small icon={Clock} label={t(label)} active={String(HOUR) === h} onclick={() => app.setSetting("hour_height", h)} />
            {/each}
          </RibbonStack>
        </RibbonGroup>
      {:else}
        <RibbonGroup label={t("ribbon.g_new")}>
          <RibbonButton icon={CalendarPlus} label={t("calendar.new_event")} onclick={() => newEvent()} />
          <RibbonButton icon={Users} label={t("calendar.new_meeting")} onclick={() => newEvent(undefined, { attendees: "", title: "" })} />
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={Plus} label={t("calendar.new_items")} menu />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content class="w-56">
              <DropdownMenu.Item onclick={() => newEvent()}><CalendarPlus class="size-4" />{t("calendar.new_event")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => newEvent(undefined, { attendees: "" })}><Users class="size-4" />{t("calendar.new_meeting")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => newEvent(toTs(current), { all_day: true, end: toTs(current) + 86400 })}><CalendarIcon class="size-4" />{t("calendar.new_all_day")}</DropdownMenu.Item>
              <DropdownMenu.Separator />
              <DropdownMenu.Item onclick={() => app.compose()}><MailPlus class="size-4" />{t("calendar.new_mail")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.newItem("contacts")}><User class="size-4" />{t("calendar.new_contact")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.newItem("tasks")}><CalendarCheck class="size-4" />{t("calendar.new_task")}</DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_goto")}>
          <RibbonButton icon={CalendarCheck} label={t("common.today")} onclick={() => { current = startOfDay(new Date()); if (view === "next7") setView("week"); }} />
          <RibbonButton icon={CalendarRange} label={t("calendar.next7")} active={view === "next7"} onclick={() => { current = startOfDay(new Date()); setView("next7"); }} />
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_arrange")}>
          {#each VIEWS.slice(0, 5) as [v, label, Icon] (v)}
            <RibbonButton icon={Icon} label={t(label)} active={view === v} onclick={() => setView(v)} />
          {/each}
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_manage_calendars")}>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={FolderOpen} label={t("calendar.open_calendar")} menu />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content class="w-60">
              <DropdownMenu.Item onclick={() => openPicker("other")}><BookUser class="size-4" />{t("calendar.open_from_book")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => openPicker("other", "rooms")}><DoorOpen class="size-4" />{t("calendar.from_rooms")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => addInternet("other")}><Globe class="size-4" />{t("calendar.from_internet")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => openPicker("shared")}><Users class="size-4" />{t("calendar.open_shared")}</DropdownMenu.Item>
              <DropdownMenu.Separator />
              <DropdownMenu.Item onclick={addCalendar}><Plus class="size-4" />{t("calendar.create_blank")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={importIcs}><Upload class="size-4" />{t("calendar.open_from_file")}</DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_share")}>
          <RibbonButton icon={MailPlus} label={t("calendar.email_calendar")} onclick={emailCalendar} />
          <RibbonButton icon={Download} label={t("calendar.save_calendar")} onclick={exportIcs} />
          <RibbonButton icon={Printer} label={t("common.print")} onclick={printAgenda} />
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_find")} last>
          <div class="flex flex-col gap-1 px-1 py-1">
            <input class="h-6 w-36 rounded-sm border px-1.5 text-[12px] outline-none focus:border-primary" placeholder={t("calendar.find_person")} bind:value={personQuery} onkeydown={findPerson} />
            <RibbonButton small icon={BookUser} label={t("compose.address_book")} onclick={() => (app.module = "contacts")} />
          </div>
        </RibbonGroup>
      {/if}
    {/snippet}
  </Ribbon>

  <div class="pane mx-0.5 mb-0.5 flex min-h-0 flex-1 flex-col">
  <div class="flex min-h-0 flex-1">
    <!-- Sol bölme: iki aylık Tarih Gezgini ve takvim listesi -->
    <div class="min-w-0 shrink-0 overflow-x-hidden overflow-y-auto border-r" style="width:{sideW}px">
      <MiniMonth month={navMonth} rangeStart={range.start} rangeDays={view === "month" ? 0 : range.days} {busyDays} onpick={(d) => (current = startOfDay(d))} onprev={() => (navMonth = addMonths(navMonth, -1))} onnext={() => (navMonth = addMonths(navMonth, 1))} />
      <MiniMonth month={addMonths(navMonth, 1)} rangeStart={range.start} rangeDays={view === "month" ? 0 : range.days} {busyDays} onpick={(d) => (current = startOfDay(d))} />
      <div class="mx-3 my-2 border-t"></div>
      <div class="px-1 pb-2 text-[13px]">
        {#each allGroups as grp, gi (grp.id)}
          {@const g = grp.id}
          {@const list = groupCalendars(g)}
          {#if g === "" || g !== "team" || list.length}
            {@const all = list.length > 0 && list.every((c) => c.visible)}
            {@const some = list.some((c) => c.visible)}
            <ContextMenu.Root>
              <ContextMenu.Trigger>
                <div class="group/grp flex items-center gap-1 py-1 pr-1 {gi ? 'mt-2' : ''}">
                  <button class="hover:bg-accent rounded p-0.5" onclick={() => (groupOpen = { ...groupOpen, [g]: !groupOpen[g] })} aria-label={grp.name}>
                    {#if groupOpen[g]}<ChevronDown class="size-3.5" />{:else}<ChevronRight class="size-3.5" />{/if}
                  </button>
                  <input type="checkbox" class="size-3.5" checked={all} indeterminate={some && !all} disabled={!list.length} onclick={(e) => { e.preventDefault(); void setGroupVisible(g, !all); }} />
                  <span class="min-w-0 flex-1 truncate text-[14px]" title={grp.name}>{grp.name}</span>
                  {#if g === ""}
                    <button class="hover:bg-accent rounded p-0.5" title={t("calendar.new_calendar")} onclick={addCalendar}><Plus class="size-4" /></button>
                  {:else if g !== "team"}
                    <button class="hover:bg-accent rounded p-0.5 opacity-0 group-hover/grp:opacity-100" title={t("calendar.open_from_book")} onclick={() => openPicker(g)}><Plus class="size-4" /></button>
                  {/if}
                </div>
              </ContextMenu.Trigger>
              <!-- Outlook takvim grubu menüsü -->
              <ContextMenu.Content class="w-60">
                <ContextMenu.Item disabled={!grp.custom} onclick={() => renameGroup(grp)}>{t("calendar.rename_group")}</ContextMenu.Item>
                <ContextMenu.Item disabled={!grp.custom} onclick={() => deleteGroup(grp)}>{t("calendar.delete_group")}</ContextMenu.Item>
                <ContextMenu.Item disabled={g === "" || !list.some((c) => c.kind === "remote" || c.kind === "internet" || g !== "")} onclick={() => removeAll(g)}>{t("calendar.remove_all")}</ContextMenu.Item>
                <ContextMenu.Separator />
                <ContextMenu.Item onclick={newGroup}>{t("calendar.new_group")}</ContextMenu.Item>
                <ContextMenu.Sub>
                  <ContextMenu.SubTrigger><Plus class="size-4 text-emerald-600" />{t("calendar.add_calendar")}</ContextMenu.SubTrigger>
                  <ContextMenu.SubContent class="w-60">
                    <ContextMenu.Item onclick={() => openPicker(g || "")}><BookUser class="size-4" />{t("calendar.open_from_book")}</ContextMenu.Item>
                    <ContextMenu.Item onclick={() => openPicker(g || "", "rooms")}><DoorOpen class="size-4" />{t("calendar.from_rooms")}</ContextMenu.Item>
                    <ContextMenu.Item onclick={() => addInternet(g || "")}><Globe class="size-4" />{t("calendar.from_internet")}</ContextMenu.Item>
                    <ContextMenu.Item onclick={() => openPicker(g === "" ? "shared" : g)}><Users class="size-4" />{t("calendar.open_shared")}</ContextMenu.Item>
                  </ContextMenu.SubContent>
                </ContextMenu.Sub>
                <ContextMenu.Separator />
                <ContextMenu.Item disabled={list.length < 2} onclick={() => sortGroupByName(g)}>{t("calendar.sort_by_name")}</ContextMenu.Item>
                <ContextMenu.Item disabled={!list.length || all} onclick={() => setGroupVisible(g, true)}>{t("calendar.show_all")}</ContextMenu.Item>
                <ContextMenu.Item disabled={!list.some((c) => c.kind === "remote" || c.kind === "internet")} onclick={async () => { remoteErrors = {}; await api.refreshInternetCalendars(true).catch(() => 0); await load(); }}>{t("calendar.refresh_fb")}</ContextMenu.Item>
                <ContextMenu.Separator />
                <ContextMenu.Item disabled={gi === 0} onclick={() => moveGroup(g, -1)}><ChevronUp class="size-4 text-primary" />{t("calendar.move_up")}</ContextMenu.Item>
                <ContextMenu.Item disabled={gi === allGroups.length - 1} onclick={() => moveGroup(g, 1)}><ChevronDown class="size-4 text-primary" />{t("calendar.move_down")}</ContextMenu.Item>
              </ContextMenu.Content>
            </ContextMenu.Root>
            {#if groupOpen[g]}
              {#each list as c, ci (c.id)}
                <ContextMenu.Root>
                  <ContextMenu.Trigger>
                    <!-- Sağ tık işareti değiştirmez: yalnızca sol tık ve onay kutusu açıp kapatır -->
                    <div
                      role="checkbox"
                      aria-checked={c.visible}
                      tabindex="0"
                      class="hover:bg-accent flex cursor-default items-center gap-2 py-0.5 pr-1 pl-7 {c.visible ? 'bg-[#cfe4fa]/60 dark:bg-primary/20' : ''}"
                      title={c.owner || c.name}
                      onclick={(e) => { if (e.button === 0) void toggleCalendar(c); }}
                      onkeydown={(e) => { if (e.key === " " || e.key === "Enter") { e.preventDefault(); void toggleCalendar(c); } }}
                    >
                      <input type="checkbox" tabindex="-1" class="pointer-events-none size-3.5" checked={c.visible} style="accent-color:{c.color}" />
                      <span class="min-w-0 flex-1 truncate">{#if c.visible}<b>{calName(c)}</b>{:else}{calName(c)}{/if}{#if c.account_id && !c.kind}<span class="text-muted-foreground"> - {c.name}</span>{/if}</span>
                      {#if remoteErrors[c.id] && c.visible}<span title={remoteErrors[c.id]}><TriangleAlert class="size-3.5 text-amber-600" /></span>{/if}
                      <span class="size-2.5 shrink-0 rounded-sm" style="background:{c.color}"></span>
                    </div>
                  </ContextMenu.Trigger>
                  <ContextMenu.Content class="w-56">
                    <ContextMenu.Item onclick={() => toggleCalendar(c)}>{c.visible ? t("calendar.hide_calendar") : t("calendar.open_cal_menu")}</ContextMenu.Item>
                    {#if c.kind !== "remote" && c.kind !== "birthdays"}<ContextMenu.Item onclick={() => renameCalendar(c)}>{t("calendar.rename_calendar")}</ContextMenu.Item>{/if}
                    {#if c.kind === "remote" || c.kind === "internet"}
                      <ContextMenu.Item disabled={c.grp === "team"} class="text-destructive" onclick={async () => { await api.deleteCalendar(c.id); load(); }}>{t("calendar.remove_calendar")}</ContextMenu.Item>
                    {:else if c.kind !== "birthdays"}
                      <ContextMenu.Item class="text-destructive" onclick={() => deleteCalendar(c)}>{t("calendar.delete_calendar_menu")}</ContextMenu.Item>
                    {/if}
                    <ContextMenu.Separator />
                    <ContextMenu.Sub>
                      <ContextMenu.SubTrigger><span class="size-3.5 rounded-sm" style="background:{c.color}"></span>{t("calendar.color")}</ContextMenu.SubTrigger>
                      <ContextMenu.SubContent class="w-52 p-2">
                        <div class="grid grid-cols-7 gap-1.5">
                          {#each PALETTE as col (col)}
                            <ContextMenu.Item class="size-6 justify-center rounded-full p-0 {c.color === col ? 'ring-2 ring-offset-1 ring-[#1b1b1b] dark:ring-white' : ''}" style="background:{col}" onclick={() => setColor(c, col)}><span class="sr-only">{col}</span></ContextMenu.Item>
                          {/each}
                        </div>
                      </ContextMenu.SubContent>
                    </ContextMenu.Sub>
                    {#if c.grp !== "team"}
                      <ContextMenu.Sub>
                        <ContextMenu.SubTrigger>{t("calendar.move_to")}</ContextMenu.SubTrigger>
                        <ContextMenu.SubContent class="w-56">
                          {#each allGroups.filter((x) => x.id !== (c.grp ?? "") && x.id !== "team") as tg (tg.id)}
                            <ContextMenu.Item onclick={() => moveToGroup(c, tg.id)}>{tg.name}</ContextMenu.Item>
                          {/each}
                        </ContextMenu.SubContent>
                      </ContextMenu.Sub>
                    {/if}
                    {#if c.owner}<ContextMenu.Item onclick={(e) => people.openCard(c.owner ?? "", e as unknown as MouseEvent)}>{t("card.person")}</ContextMenu.Item>{/if}
                    <ContextMenu.Separator />
                    <ContextMenu.Item disabled={ci === 0} onclick={() => moveCalendar(c, -1)}><ChevronUp class="size-4 text-primary" />{t("calendar.move_up")}</ContextMenu.Item>
                    <ContextMenu.Item disabled={ci === list.length - 1} onclick={() => moveCalendar(c, 1)}><ChevronDown class="size-4 text-primary" />{t("calendar.move_down")}</ContextMenu.Item>
                  </ContextMenu.Content>
                </ContextMenu.Root>
              {/each}
            {/if}
          {/if}
        {/each}
      </div>
    </div>
    <Splitter bind:size={sideW} min={180} max={600} onend={(v) => app.setSetting("pane_cal_w", String(v))} />
    <div class="min-w-0 flex-1">
      <div class="flex h-full min-h-0 flex-col">
        <!-- Outlook gezinti çubuğu: Bugün  <  >  12 - 18 Ekim 2026 ............ Hafta ▾ -->
        <div class="flex items-center gap-1.5 px-3 py-2.5">
          <button class="hover:bg-accent h-7 rounded border px-3 text-[12px]" onclick={() => (current = startOfDay(new Date()))}>{t("common.today")}</button>
          <button class="hover:bg-accent flex size-7 items-center justify-center rounded border" aria-label="‹" onclick={() => move(-1)}><ChevronLeft class="size-4" /></button>
          <button class="hover:bg-accent flex size-7 items-center justify-center rounded border" aria-label="›" onclick={() => move(1)}><ChevronRight class="size-4" /></button>
          <h1 class="ml-3 text-[19px] font-semibold capitalize">{searchResults ? t("calendar.search_results", app.search) : title}</h1>
          <span class="flex-1"></span>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>
              {#snippet child({ props })}<button {...props} class="hover:bg-accent flex h-7 items-center gap-2 rounded px-2 text-[12px]"><CalendarFold class="size-4" />{t(VIEWS.find((x) => x[0] === view)?.[1] ?? "calendar.next7")}<ChevronDown class="size-3.5" /></button>{/snippet}
            </DropdownMenu.Trigger>
            <DropdownMenu.Content align="end" class="w-48">
              <DropdownMenu.RadioGroup value={view} onValueChange={(v) => setView(v as View)}>
                {#each VIEWS as [v, label] (v)}<DropdownMenu.RadioItem value={v}>{t(label)}</DropdownMenu.RadioItem>{/each}
              </DropdownMenu.RadioGroup>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </div>

        {#if searchResults}
          <div class="flex-1 overflow-y-auto border-t">
            {#each searchResults as e (e.id)}
              <button class="hover:bg-accent flex w-full gap-3 border-b px-4 py-2 text-left text-sm" onclick={() => openEvent(e, null)}>
                <span class="w-1 rounded" style="background:{evColor(e)}"></span>
                <span class="w-48 shrink-0">{fmtDate(e.start, { dateStyle: "medium" })} {e.all_day ? "" : fmtTime(e.start)}</span>
                <span class="font-medium">{e.title}</span><span class="text-muted-foreground">{e.location}</span>
              </button>
            {:else}
              <div class="text-muted-foreground p-8 text-center text-sm">{t("common.no_results")}</div>
            {/each}
          </div>
        {:else if view === "month"}
          <div class="grid flex-1 grid-cols-7 grid-rows-[auto_repeat(6,minmax(0,1fr))] overflow-hidden border-t">
            {#each days.slice(0, 7) as d (d.getTime())}<div class="border-b border-l px-2 py-1 text-[13px] capitalize">{weekdayLong(d)}</div>{/each}
            {#each days as d (d.getTime())}
              {@const list = occurrences.filter((o) => o.start < toTs(d) + 86400 && o.end > toTs(d))}
              {@const isToday = sameDay(d, new Date())}
              <div
                role="gridcell"
                tabindex="-1"
                class="min-h-0 overflow-hidden border-b border-l p-0.5 {isToday ? 'bg-[#e5f1fb] dark:bg-primary/15' : d.getMonth() !== current.getMonth() ? 'bg-muted/40' : 'bg-background'}"
                ondblclick={() => newEvent(toTs(d) + workStart * 3600, {})}
              >
                <button class="mb-0.5 px-1 text-[12px] {isToday ? 'text-primary font-bold' : 'hover:underline'}" onclick={() => { current = d; setView("day"); }}>{d.getDate() === 1 ? fmtDate(d, { day: "numeric", month: "short" }) : d.getDate()}</button>
                {#each list.slice(0, 4) as o (occKey(o))}
                  {@const lk = look(o.event)}
                  <button
                    class="relative mb-px block h-[18px] w-full overflow-hidden pr-1 pl-2.5 text-left text-[11px] whitespace-nowrap {o.event.is_cancelled ? '' : ''}"
                    style="background:{lk.fill};border:1px {lk.dashed ? 'dashed' : 'solid'} {lk.border}"
                    onclick={() => (selectedOcc = occKey(o))}
                    ondblclick={() => openEvent(o.event, o.start)}
                    onmouseenter={(e) => tipEnter(e, o)}
                    onmousemove={tipMove}
                    onmouseleave={tipLeave}
                  >
                    <span class="absolute inset-y-0 left-0 w-[5px]" style="background:{lk.bar}"></span>
                    {#if !o.event.all_day}{fmtTime(o.start)} {/if}<b>{o.event.title}</b>
                  </button>
                {/each}
                {#if list.length > 4}<button class="text-muted-foreground px-1 text-[11px] hover:underline" onclick={() => { current = d; setView("day"); }}>+{list.length - 4} {t("calendar.more")}</button>{/if}
              </div>
            {/each}
          </div>
        {:else if view === "agenda"}
          <div class="flex-1 overflow-y-auto border-t">
            {#each days as d (d.getTime())}
              {@const list = occurrences.filter((o) => o.start < toTs(d) + 86400 && o.end > toTs(d))}
              {#if list.length}
                <div class="bg-muted/50 sticky top-0 px-4 py-1 text-xs font-semibold capitalize">{fmtDateLong(d)}</div>
                {#each list as o (occKey(o))}
                  <button class="hover:bg-accent flex w-full items-center gap-3 border-b px-4 py-2 text-left text-sm" ondblclick={() => openEvent(o.event, o.start)} onclick={() => openEvent(o.event, o.start)}>
                    <span class="h-8 w-1" style="background:{look(o.event).bar}"></span>
                    <span class="text-muted-foreground w-28 shrink-0">{o.event.all_day ? t("calendar.all_day") : `${fmtTime(o.start)} – ${fmtTime(o.end)}`}</span>
                    <span class="flex-1"><span class="font-medium {o.event.is_cancelled ? 'line-through opacity-70' : ''}">{o.event.title}</span>{#if o.event.location}<span class="text-muted-foreground"> · {o.event.location}</span>{/if}{#if o.event.organizer}<span class="text-muted-foreground"> · {displayName(o.event.organizer)}</span>{/if}</span>
                    {#if o.event.recurrence !== "none" || o.event.master_id}<Repeat class="text-muted-foreground size-3.5" />{/if}
                    {#if o.event.attendees}<Users class="text-muted-foreground size-3.5" />{/if}
                  </button>
                {/each}
              {/if}
            {/each}
            {#if !occurrences.some((o) => o.start >= toTs(range.start))}<div class="text-muted-foreground p-8 text-center text-sm">{t("calendar.no_events")}</div>{/if}
          </div>
        {:else if view === "schedule"}
          <!-- Zamanlama Görünümü: satırlar takvimler, sütunlar saatler (yatay) -->
          {@const dayStart = toTs(current)}
          <div bind:this={schedEl} class="min-h-0 flex-1 overflow-auto border-t">
            <div class="relative" style="width:{200 + 24 * SCHED_W}px">
              <div class="bg-background sticky top-0 z-20 flex border-b">
                <div class="bg-background sticky left-0 z-10 w-[200px] shrink-0 border-r px-2 py-1 text-[12px] font-semibold capitalize">{fmtDateLong(current)}</div>
                {#each Array(24) as _, h (h)}
                  <div class="shrink-0 border-l px-1 py-1 text-[11px] {h === new Date().getHours() && sameDay(current, new Date()) ? 'font-bold' : ''}" style="width:{SCHED_W}px">{fmtTime(new Date(2024, 0, 1, h))}</div>
                {/each}
              </div>
              {#each calendars.filter((c) => c.visible) as c (c.id)}
                {@const list = occurrences.filter((o) => o.event.calendar_id === c.id && o.start < dayStart + 86400 && o.end > dayStart)}
                <div class="flex border-b" style="height:52px">
                  <div class="bg-background sticky left-0 z-10 flex w-[200px] shrink-0 items-center gap-2 border-r px-2 text-[13px]"><span class="size-2.5 shrink-0 rounded-sm" style="background:{c.color}"></span><span class="truncate">{c.name}</span></div>
                  <div class="relative shrink-0" style="width:{24 * SCHED_W}px" role="gridcell" tabindex="-1" ondblclick={(e) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); newEvent(dayStart + Math.floor(((e.clientX - r.left) / SCHED_W) * 2) * 1800); }}>
                    {#each Array(24) as _, h (h)}
                      <div class="absolute inset-y-0 border-l {h < workStart || h >= workEnd ? 'bg-muted/40' : ''}" style="left:{h * SCHED_W}px;width:{SCHED_W}px"></div>
                    {/each}
                    {#each list as o (occKey(o))}
                      {@const lk = look(o.event)}
                      {@const l = Math.max(0, (o.start - dayStart) / 3600) * SCHED_W}
                      {@const r = Math.min(24, (o.end - dayStart) / 3600) * SCHED_W}
                      <button
                        class="absolute top-1.5 bottom-1.5 z-10 overflow-hidden pr-1 text-left text-[11px] {selectedOcc === occKey(o) ? 'ring-2 ring-[#1b1b1b] dark:ring-white' : ''}"
                        style="left:{l}px;width:{Math.max(8, r - l - 1)}px;background:{lk.fill};border:1px {lk.dashed ? 'dashed' : 'solid'} {lk.border}"
                        onclick={() => (selectedOcc = occKey(o))}
                        ondblclick={(e) => { e.stopPropagation(); openEvent(o.event, o.start); }}
                        onmouseenter={(e) => tipEnter(e, o)}
                        onmousemove={tipMove}
                        onmouseleave={tipLeave}
                      >{@render eventBox(o, true)}</button>
                    {/each}
                    {#if sameDay(current, new Date())}
                      <div class="pointer-events-none absolute inset-y-0 z-20 w-px bg-[#1b1b1b] dark:bg-white" style="left:{((now.ts - dayStart) / 3600) * SCHED_W}px"></div>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {:else}
          <!-- Gün / hafta görünümü (Outlook): gün adları ve tarih, bugünün sütunu çerçeveli -->
          {@const colsTemplate = `48px repeat(${days.length}, minmax(0, 1fr))`}
          {@const todayIdx = days.findIndex((d) => sameDay(d, new Date()))}
          <div class="grid border-t pr-[var(--sbw,0px)]" style="grid-template-columns:{colsTemplate}">
            <div></div>
            {#each days as d, i (d.getTime())}
              {@const isToday = i === todayIdx}
              <button class="border-l px-2 pt-1.5 pb-1 text-left {isToday ? 'bg-[#e5f1fb] dark:bg-primary/20' : ''}" style={isToday ? "border:1px solid var(--primary);border-top-width:2px;border-bottom:0" : ""} onclick={() => { current = d; setView("day"); }}>
                <div class="text-[14px] capitalize {isToday ? 'font-semibold' : ''}">{weekdayLong(d)}</div>
                <div class="text-[14px] {isToday ? 'font-bold' : ''}">{d.getDate()}</div>
              </button>
            {/each}
            <div class="border-b"></div>
            {#each days as d, i (d.getTime())}
              <div role="gridcell" tabindex="-1" class="min-h-[30px] space-y-px border-b border-l p-0.5 {i === todayIdx ? 'bg-[#e5f1fb]/60 dark:bg-primary/10' : ''}" style={i === todayIdx ? "border-left:1px solid var(--primary);border-right:1px solid var(--primary)" : ""} ondblclick={() => newEvent(toTs(d), { all_day: true, end: toTs(d) + 86400 })}>
                {#each dayOccurrences(d, true) as o (occKey(o))}
                  {@const lk = look(o.event)}
                  <button
                    class="relative block h-[20px] w-full overflow-hidden pr-1 text-left text-[11px] {selectedOcc === occKey(o) ? 'ring-2 ring-[#1b1b1b] dark:ring-white' : ''}"
                    style="background:{lk.fill};border:1px {lk.dashed ? 'dashed' : 'solid'} {lk.border}"
                    onclick={() => (selectedOcc = occKey(o))}
                    ondblclick={() => openEvent(o.event, o.start)}
                    onmouseenter={(e) => tipEnter(e, o)}
                    onmousemove={tipMove}
                    onmouseleave={tipLeave}
                  >{@render eventBox(o, true)}</button>
                {/each}
              </div>
            {/each}
          </div>
          <div bind:this={gridEl} class="relative min-h-0 flex-1 overflow-y-scroll">
            <div class="relative grid" style="grid-template-columns:{colsTemplate};height:{24 * HOUR}px">
              <div class="relative">
                {#each Array(24) as _, h (h)}
                  <div class="text-muted-foreground absolute right-1.5 left-0 border-t pt-0.5 text-right text-[11px] {h === new Date().getHours() ? 'text-foreground font-bold' : ''}" style="top:{h * HOUR}px">{fmtTime(new Date(2024, 0, 1, h))}</div>
                {/each}
              </div>
              {#each days as d, di (d.getTime())}
                {@const dayStart = toTs(d)}
                {@const isToday = di === todayIdx}
                <div class="relative border-l" style={isToday ? "border-left:1px solid var(--primary);border-right:1px solid var(--primary)" : ""}>
                  <!-- saat ve yarım saat çizgileri; çalışma saatleri dışı gri -->
                  {#each Array(24) as _, h (h)}
                    {@const off = h < workStart || h >= workEnd || d.getDay() === 0 || d.getDay() === 6}
                    <div class="absolute right-0 left-0 border-t {off ? 'bg-[#f3f3f3] dark:bg-white/[0.03]' : isToday ? 'bg-[#f3f9fd] dark:bg-primary/[0.06]' : ''}" style="top:{h * HOUR}px;height:{HOUR}px">
                      <div class="absolute right-0 left-0 border-t border-dotted border-black/10 dark:border-white/10" style="top:{HOUR / 2}px"></div>
                    </div>
                  {/each}
                  <div role="gridcell" tabindex="-1" class="absolute inset-0" ondblclick={(e) => slotClick(e, d)} onclick={() => (selectedOcc = null)}></div>
                  {#each layout(dayOccurrences(d, false), dayStart) as p (occKey(p.o))}
                    {@const dragging = drag && drag.o === p.o}
                    {@const extra = dragging ? (drag!.mode === "move" ? drag!.dy : 0) : 0}
                    {@const extraH = dragging && drag!.mode === "resize" ? drag!.dy : 0}
                    {@const lk = look(p.o.event)}
                    {@const sel = selectedOcc === occKey(p.o)}
                    <div
                      role="button"
                      tabindex="0"
                      class="absolute z-10 overflow-hidden pt-0.5 pr-1 pb-3 text-[11px] text-[#1f1f1f] dark:text-foreground {dragging ? 'opacity-80' : ''} {sel ? 'z-[15] outline-2 outline-[#1b1b1b] dark:outline-white' : ''}"
                      style="top:{p.top + extra}px;height:{Math.max(18, p.height + extraH - 1)}px;left:calc({(p.col / p.cols) * 100}% + 1px);width:calc({100 / p.cols}% - 4px);background:{lk.fill};border:1px {lk.dashed ? 'dashed' : 'solid'} {lk.border};transform:translateX({dragging ? drag!.dayShift * 100 * p.cols : 0}%)"
                      onpointerdown={(e) => { selectedOcc = occKey(p.o); tipLeave(); startDrag(e, p.o, "move", (e.currentTarget as HTMLElement).parentElement!.clientWidth); }}
                      ondblclick={(e) => { e.stopPropagation(); openEvent(p.o.event, p.o.start); }}
                      onkeydown={(e) => e.key === "Enter" && openEvent(p.o.event, p.o.start)}
                      onmouseenter={(e) => tipEnter(e, p.o)}
                      onmousemove={tipMove}
                      onmouseleave={tipLeave}
                    >
                      {@render eventBox(p.o, p.height < 34)}
                      <div role="separator" class="absolute right-0 bottom-0 left-0 h-1.5 cursor-ns-resize" onpointerdown={(e) => startDrag(e, p.o, "resize", 1)}></div>
                    </div>
                  {/each}
                </div>
              {/each}
              <!-- Şimdiki zaman: haftanın başından bugüne siyah çizgi, bugünün başında nokta -->
              {#if todayIdx >= 0}
                {@const y = ((now.ts - toTs(days[todayIdx])) / 3600) * HOUR}
                <div class="pointer-events-none absolute z-20 h-px bg-[#1b1b1b] dark:bg-white" style="top:{y}px;left:48px;width:calc((100% - 48px) * {(todayIdx + 1) / days.length})"></div>
                <div class="pointer-events-none absolute z-20 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[#1b1b1b] dark:bg-white" style="top:{y}px;left:calc(48px + (100% - 48px) * {todayIdx / days.length})"></div>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
  </div>
</div>

<!-- Üzerine gelince ayrıntı (Outlook ipucu kartı) -->
{#if tip}
  {@const ev = tip.o.event}
  {@const att = [...splitAddresses(ev.attendees ?? ""), ...splitAddresses(ev.optional_attendees ?? "")]}
  <div
    class="bg-popover text-popover-foreground pointer-events-none fixed z-[85] w-80 border p-3 text-[12px] shadow-lg"
    style="left:{Math.min(tip.x + 14, window.innerWidth - 336)}px;top:{Math.min(tip.y + 16, window.innerHeight - 230)}px"
  >
    <div class="flex items-start gap-2">
      <span class="mt-0.5 h-10 w-1.5 shrink-0" style="background:{look(ev).bar};border:1px solid {look(ev).border}"></span>
      <div class="min-w-0">
        <div class="text-[14px] leading-tight font-semibold break-words">{ev.title || t("calendar.untitled")}</div>
        <div class="text-muted-foreground mt-1 flex items-center gap-1.5"><Clock class="size-3.5 shrink-0" />
          {fmtDate(tip.o.start, { weekday: "long", day: "numeric", month: "long" })}
          {#if ev.all_day}· {t("calendar.all_day")}{:else}{fmtTime(tip.o.start)} – {fmtTime(tip.o.end)}{/if}
        </div>
      </div>
    </div>
    <div class="mt-2 space-y-1">
      {#if ev.id < 0}<div class="text-muted-foreground flex items-center gap-1.5"><CalendarIcon class="size-3.5 shrink-0" />{calendars.find((c) => c.id === ev.calendar_id)?.name}</div>{/if}
      {#if ev.location}<div class="flex items-start gap-1.5"><MapPin class="mt-px size-3.5 shrink-0" /><span class="break-words">{ev.location}</span></div>{/if}
      {#if ev.organizer}<div class="flex items-start gap-1.5"><User class="mt-px size-3.5 shrink-0" /><span>{t("calendar.organizer")}: {displayName(ev.organizer)}</span></div>{/if}
      {#if att.length}<div class="flex items-start gap-1.5"><Users class="mt-px size-3.5 shrink-0" /><span class="line-clamp-2">{att.slice(0, 4).map(displayName).join("; ")}{att.length > 4 ? ` ${t("calendar.and_n_more", att.length - 4)}` : ""}</span></div>{/if}
      {#if ev.recurrence !== "none" || ev.master_id}<div class="flex items-center gap-1.5"><Repeat class="size-3.5 shrink-0" />{t("calendar.recurring")}</div>{/if}
      {#if ev.response && RESP[ev.response]}<div class="text-muted-foreground">{t(RESP[ev.response])}</div>{/if}
      {#if ev.is_cancelled}<div class="font-semibold text-red-700 dark:text-red-400">{t("calendar.cancelled")}</div>{/if}
      <div class="text-muted-foreground">{t("calendar.show_as")}: {t(`showas.${ev.show_as}`)}</div>
      {#if ev.description}<div class="text-muted-foreground mt-1 line-clamp-3 border-t pt-1 whitespace-pre-line">{ev.description.slice(0, 300)}</div>{/if}
    </div>
  </div>
{/if}

<!-- Takvim Ekle: Outlook "Ad Seç" penceresi; çift tıklanan kişiler alttaki Takvim alanına eklenir -->
<AddressBookPicker bind:open={picker.open} source={picker.source} fieldLabel={t("calendar.calendar")} limit={MAX_CALENDARS} onpick={addRemoteCalendars} />

<EventDialog bind:open={dialogOpen} event={editing} occurrenceStart={editingOcc} {calendars} onsaved={load} readonly={editingReadonly} />
