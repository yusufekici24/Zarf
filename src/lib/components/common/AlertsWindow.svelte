<script lang="ts">
  // Masaüstü yeni posta uyarıları (Outlook/Skype gibi): ekranın sağ altında, üst üste dizilen
  // dikdörtgen kartlar. Kullanıcının dikkatini dağıtmamak için en fazla ekranın yarısı
  // kullanılır ve yalnızca tamamı sığan kartlar gösterilir (yarım kart olmaz). Fazlası sırada
  // bekler: gösterilen bir kart kapanınca (süre, ×, tıklama) sıradaki en alta gelir.
  // Kart birkaç saniye sonra kaybolur, üzerine gelindiğinde bekler. Tıklayınca ileti açılır.
  import { onMount, untrack } from "svelte";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { emitTo, listen } from "@tauri-apps/api/event";
  import { setMode } from "mode-watcher";
  import Avatar from "./Avatar.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { i18n, t } from "$lib/i18n.svelte";
  import { fmtTime } from "$lib/format";
  import { applyScale, currentScale } from "$lib/zoom";
  import X from "@lucide/svelte/icons/x";
  import Flag from "@lucide/svelte/icons/flag";
  import MailOpen from "@lucide/svelte/icons/mail-open";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import AlertCircle from "@lucide/svelte/icons/circle-alert";
  import Reply from "@lucide/svelte/icons/reply";
  import ReplyAll from "@lucide/svelte/icons/reply-all";
  import Forward from "@lucide/svelte/icons/forward";
  import Archive from "@lucide/svelte/icons/archive";
  import BellOff from "@lucide/svelte/icons/bell-off";
  import Video from "@lucide/svelte/icons/video";
  import AlarmClock from "@lucide/svelte/icons/alarm-clock";
  import { ALERT_ACTIONS, alertButtons, type AlertAction } from "$lib/alerts";

  type Item = {
    id: number;
    subject: string;
    sender: string;
    email?: string;
    preview?: string;
    date?: number;
    importance?: number;
    has_attachments?: boolean;
    key: number;
    left: number;
    /** Toplantı/görev anımsatıcısı kartı (yoksa yeni posta) */
    type?: "reminder";
    rkey?: string;
    kind?: "event" | "task";
    item_id?: number;
    title?: string;
    location?: string;
    start?: number;
    end?: number;
    all_day?: boolean;
    join_url?: string;
  };
  /** Anımsatıcı kartı biraz daha uzun durur (üzerine gelince bekler); anımsatıcı
      penceresi ayrıca açık kalır */
  const REMINDER_LIFE = 30;

  const CARD = 172; // kart yüksekliği
  const GAP = 8;
  const PAD = 10; // gölge için kenar boşluğu
  /** Pencere ekranın sağ kenarına dayalı; kart ile ekran kenarı arasındaki boşluk */
  const EDGE = 12;
  let items = $state<Item[]>([]);
  /** Sığmadığı için sırada bekleyenler (gelme sırasıyla) */
  let queue: Omit<Item, "key" | "left">[] = [];
  /** Ekranın yarısına tamamı sığan kart sayısı */
  let maxCards = $state(3);
  let hovered = $state<number | null>(null);
  let seq = 1;
  const LIFE = 7; // saniye
  const ANIM = 260; // ms
  /** Kapanan kartın sağa kayarak çıkış süresi */
  const OUT_ANIM = 560;

  /** Boş yer oldukça sıradakileri en alta ekler */
  function fill(list = items) {
    const life = Number(app.setting("alert_seconds", String(LIFE))) || LIFE;
    const next = [...list];
    while (next.length < maxCards && queue.length) {
      const it = queue.shift()!;
      // Aynı anımsatıcının eski kartı ("yaklaşıyor") yenisiyle ("başladı") değişir
      if (it.type === "reminder") {
        const i = next.findIndex((x) => x.rkey === it.rkey);
        if (i >= 0) next.splice(i, 1);
      }
      next.push({ ...it, key: seq++, left: it.type === "reminder" ? REMINDER_LIFE : life });
    }
    items = next;
  }

  /** Kart eklenmeden önce ayarlar tazelenir: düğmeler/süre her zaman güncel ayarla gelir */
  async function add(list: Omit<Item, "key" | "left">[]) {
    if (!list.length) return;
    await app.loadAll().catch(() => {});
    queue.push(...list);
    fill();
  }

  function remove(key: number) {
    fill(items.filter((i) => i.key !== key));
  }

  // Pencere kartlar görünürken sabit boyutta ve sabit yerde durur (kart kapanınca yeniden
  // boyutlanıp konumlanmaz: bazı pencere yöneticilerinde bu, kartların hepsinin bir an
  // kaybolmasına yol açıyordu). Yalnızca kartların bulunduğu alan tıklamaları alır; boş
  // saydam kısım alttaki pencerelere geçer. Ölçüler CSS pikseli × sayfa yakınlaştırması.
  const stackH = (n: number) => (n ? n * CARD + (n - 1) * GAP : 0);
  const winH = $derived(stackH(maxCards) + PAD * 2);
  let shown = false;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const n = items.length;
    const z = currentScale();
    const h = winH;
    untrack(() => {
      clearTimeout(hideTimer);
      if (n && !shown) {
        shown = true;
        void invoke("alerts_layout", { height: Math.ceil(h * z) }).catch(() => {});
      } else if (!n && shown) {
        // Çıkış animasyonu bitince gizlenir
        hideTimer = setTimeout(() => {
          shown = false;
          void invoke("alerts_layout", { height: 0 }).catch(() => {});
        }, OUT_ANIM + 40);
      }
      // Tıklanabilir alan: animasyon sürerken eski ve yeni yığının birleşimi, sonra yeni yığın
      const top = (h - PAD - stackH(n)) * z;
      void invoke("alerts_input", { top: Math.min(top, lastTop), height: h * z }).catch(() => {});
      setTimeout(() => void invoke("alerts_input", { top, height: h * z }).catch(() => {}), OUT_ANIM + 40);
      lastTop = top;
    });
  });
  let lastTop = 1e6;

  /** Kapanan kart solarak yüksekliğini de kapatır: üstündekiler birden değil yumuşakça kayar */
  /** Yeni kart ekranın sağ kenarının dışından sola doğru yumuşakça kayarak girer */
  function slideIn(_node: Element) {
    return { duration: 520, easing: cubicOut, css: (_t: number, u: number) => `transform:translateX(${u * 110}%)` };
  }

  /** Kapanan kart geldiği gibi sağ kenardan dışarı kayar, sonra yeri yumuşakça kapanır */
  function collapse(_node: Element) {
    return {
      duration: OUT_ANIM,
      css: (_t: number, u: number) => {
        const slide = cubicOut(Math.min(1, u / 0.6));
        const shrink = u < 0.6 ? 1 : 1 - cubicOut((u - 0.6) / 0.4);
        return `transform:translateX(${slide * 110}%);height:${shrink * CARD}px;margin-top:${(shrink - 1) * GAP}px`;
      },
    };
  }

  /** Çalışma alanının yarısına tamamı sığan kart sayısı */
  async function measure() {
    const avail = await invoke<number>("alerts_area").catch(() => 0);
    if (!avail) return;
    const half = avail / 2 / currentScale();
    maxCards = Math.max(1, Math.floor((half - PAD * 2 + GAP) / (CARD + GAP)));
  }

  onMount(() => {
    // Saydam pencere
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const offs: (() => void)[] = [];
    const tick = setInterval(() => {
      if (!items.length) return;
      const next = items.map((i) => (i.key === hovered ? i : { ...i, left: i.left - 1 }));
      if (next.some((i) => i.left <= 0)) fill(next.filter((i) => i.left > 0));
      else items = next;
    }, 1000);
    (async () => {
      await i18n.load();
      await app.loadAll();
      setMode((app.setting("theme", "system") as "light" | "dark" | "system") || "system");
      // Ölçek, kart sayısı hesaplanmadan önce uygulanır
      await applyScale(app.setting("ui_scale", "auto"));
      await measure();
      void add(await invoke<Omit<Item, "key" | "left">[]>("alerts_take").catch(() => []));
      offs.push(await listen<Omit<Item, "key" | "left">[]>("alerts://add", (e) => void add(e.payload)));
      // Anımsatıcı penceresinde kapatılan/ertelenen öğelerin kartları da kalkar
      offs.push(
        await listen<string[]>("alerts://reminders-done", (e) => {
          queue = queue.filter((q) => !q.rkey || !e.payload.includes(q.rkey));
          fill(items.filter((i) => !i.rkey || !e.payload.includes(i.rkey)));
        }),
      );
      // Ayarlar penceresinde düğmeler/süre/tema değişince hemen uygulanır
      offs.push(await app.listenState(() => setMode((app.setting("theme", "system") as "light" | "dark" | "system") || "system")));
    })();
    return () => {
      clearInterval(tick);
      offs.forEach((f) => f());
    };
  });

  async function open(it: Item) {
    remove(it.key);
    if (it.type === "reminder") {
      await emitTo("main", "app://open-item", { module: it.kind === "event" ? "calendar" : "tasks", id: it.item_id }).catch(() => {});
      await api.focusMain().catch(() => {});
      return;
    }
    await emitTo("main", "app://open-message", { id: it.id }).catch(() => {});
  }

  /** Anımsatıcı kartı: Kapat ya da 5 dakika ertele (anımsatıcı penceresi de güncellenir) */
  async function reminderAct(it: Item, snooze: number | null) {
    remove(it.key);
    if (!it.rkey) return;
    await api.dismissReminder(it.rkey, snooze).catch(() => {});
    await emitTo("reminders", "reminders://done", [it.rkey]).catch(() => {});
  }

  /** "10 dakika içinde başlıyor", "Toplantı başladı", "5 dakika önce başladı" */
  function reminderStatus(it: Item, _left?: number): string {
    const s = (it.start ?? 0) - Date.now() / 1000;
    const m = Math.round(Math.abs(s) / 60);
    if (it.kind !== "event") return s > 60 ? t("alerts.task_due_in", fmtTime(it.start ?? 0)) : t("alerts.task_due");
    if (s > 30) return m < 60 ? t("alerts.meeting_soon", Math.max(1, m)) : t("alerts.meeting_at", fmtTime(it.start ?? 0));
    return m < 2 ? t("alerts.meeting_started") : t("alerts.meeting_started_ago", m);
  }
  function span(it: Item): string {
    if (!it.start) return "";
    if (it.all_day) return t("reminder.all_day");
    return it.end && it.end > it.start ? `${fmtTime(it.start)} – ${fmtTime(it.end)}` : fmtTime(it.start);
  }
  const ICONS: Record<AlertAction, typeof Reply> = { reply: Reply, reply_all: ReplyAll, forward: Forward, archive: Archive, delete: Trash2, read: MailOpen, flag: Flag, mute: BellOff };
  const buttons = $derived(alertButtons((k, d) => app.setting(k, d)));

  /** Kart düğmesi: ayarlardan seçilen eylem (Ayarlar > Posta > Bildirimler) */
  async function act(it: Item, kind: AlertAction) {
    remove(it.key);
    try {
      if (kind === "reply" || kind === "reply_all" || kind === "forward") {
        app.compose({ mode: kind, sourceId: it.id });
        await api.setRead([it.id], true);
      } else if (kind === "mute") {
        // Bir saat boyunca yeni posta uyarısı gösterilmez; sıradakiler de kapanır
        await app.setSetting("alerts_muted_until", String(Math.floor(Date.now() / 1000) + 3600));
        queue = [];
        items = [];
        return;
      } else if (kind === "read") await api.setRead([it.id], true);
      else if (kind === "flag") await api.setFlagged([it.id], true);
      else if (kind === "archive") await api.moveToRole([it.id], "archive");
      else await api.deleteMessages([it.id]);
      await emitTo("main", "app://refresh-mail", {}).catch(() => {});
    } catch {
      /* yok */
    }
  }

  /** "şimdi", "3 dk" gibi geçen süre (kartlar her saniye yenilenir) */
  function ago(ts?: number, _left?: number): string {
    if (!ts) return "";
    const s = Math.floor(Date.now() / 1000) - ts;
    if (s < 60) return t("alerts.now");
    if (s < 3600) return t("alerts.min_ago", Math.floor(s / 60));
    return fmtTime(ts);
  }
</script>

<div class="flex h-screen flex-col justify-end gap-2 overflow-hidden select-none" style="padding:{PAD}px {PAD + EDGE}px {PAD}px {PAD}px">
  {#each items as it (it.key)}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      animate:flip={{ duration: ANIM }}
      in:slideIn
      out:collapse
      role="button"
      tabindex="-1"
      class="bg-popover text-popover-foreground relative flex shrink-0 cursor-pointer flex-col overflow-hidden rounded-[4px] border border-black/10 px-4 pt-3 pb-3 shadow-[0_8px_28px_rgba(0,0,0,0.18)] dark:border-white/15"
      style="height:{CARD}px"
      onmouseenter={() => (hovered = it.key)}
      onmouseleave={() => (hovered = null)}
      onclick={() => open(it)}
    >
      {#if it.type === "reminder"}
        {@const started = (it.start ?? 0) <= Date.now() / 1000 + 30 && it.kind === "event"}
        <div class="flex h-6 items-center gap-2 text-[12px]">
          <img src="/icon.svg" alt="" class="size-5 rounded-md" />
          <span class="font-semibold">Lynora</span>
          <span class="text-muted-foreground">•</span>
          <span class="text-muted-foreground truncate">{it.kind === "event" ? t("alerts.meeting") : t("alerts.task_reminder")}</span>
          <span class="flex-1"></span>
          <button class="hover:bg-accent -mr-1 rounded-md p-1" title={t("common.close")} onclick={(e) => { e.stopPropagation(); remove(it.key); }}><X class="size-4" /></button>
        </div>
        <div class="mt-1.5 flex min-h-0 flex-1 gap-3">
          <!-- Takvim yaprağı: ay ve gün -->
          <div class="flex size-12 shrink-0 flex-col overflow-hidden rounded-[4px] border border-black/10 text-center dark:border-white/15">
            <div class="{started ? 'bg-red-600' : 'bg-primary'} text-[10px] leading-4 font-semibold text-white uppercase">{new Date((it.start ?? 0) * 1000).toLocaleDateString(i18n.locale, { month: "short" })}</div>
            <div class="bg-background flex flex-1 items-center justify-center text-[18px] leading-none font-semibold">{new Date((it.start ?? 0) * 1000).getDate()}</div>
          </div>
          <div class="min-w-0 flex-1">
            <div class="truncate text-[14px] font-semibold">{it.title || t("mail.no_subject")}</div>
            <div class="text-muted-foreground truncate text-[12.5px] leading-[18px]">{span(it)}{it.location ? ` · ${it.location}` : ""}</div>
            <div class="truncate text-[12.5px] leading-[18px] font-semibold {started ? 'text-red-600' : 'text-primary'}">{reminderStatus(it, it.left)}</div>
          </div>
        </div>
        <div class="mt-2 grid gap-2" style="grid-template-columns:repeat({it.join_url ? 3 : 2},minmax(0,1fr))">
          {#if it.join_url}
            <button class="flex h-9 min-w-0 items-center justify-center gap-1 rounded-lg border border-[#5b5fc7] bg-[#5b5fc7] px-1.5 text-[12px] whitespace-nowrap text-white hover:bg-[#4f52b2]" onclick={(e) => { e.stopPropagation(); if (it.join_url) void api.openUrl(it.join_url); }}><Video class="size-3.5 shrink-0" /><span class="truncate">{t("alerts.join")}</span></button>
          {/if}
          <button class="flex h-9 min-w-0 items-center justify-center gap-1 rounded-lg border border-black/10 bg-black/[0.03] px-1.5 text-[12px] whitespace-nowrap hover:bg-black/[0.07] dark:border-white/15 dark:bg-white/5 dark:hover:bg-white/10" onclick={(e) => { e.stopPropagation(); void reminderAct(it, 5); }}><AlarmClock class="size-3.5 shrink-0" /><span class="truncate">{t("alerts.snooze5")}</span></button>
          <button class="flex h-9 min-w-0 items-center justify-center gap-1 rounded-lg border border-black/10 bg-black/[0.03] px-1.5 text-[12px] whitespace-nowrap hover:bg-black/[0.07] dark:border-white/15 dark:bg-white/5 dark:hover:bg-white/10" onclick={(e) => { e.stopPropagation(); void reminderAct(it, null); }}><BellOff class="size-3.5 shrink-0" /><span class="truncate">{t("alerts.dismiss")}</span></button>
        </div>
      {:else}
      <!-- Üst satır: uygulama, tür, zaman, kapat -->
      <div class="flex h-6 items-center gap-2 text-[12px]">
        <img src="/icon.svg" alt="" class="size-5 rounded-md" />
        <span class="font-semibold">Lynora</span>
        <span class="text-muted-foreground">•</span>
        <span class="text-muted-foreground truncate">{t("alerts.new_mail")}</span>
        <span class="flex-1"></span>
        <span class="text-muted-foreground shrink-0">{ago(it.date, it.left)}</span>
        <button class="hover:bg-accent -mr-1 rounded-md p-1" title={t("common.close")} onclick={(e) => { e.stopPropagation(); remove(it.key); }}><X class="size-4" /></button>
      </div>
      <!-- Gönderen, konu, önizleme -->
      <div class="mt-1.5 flex min-h-0 flex-1 gap-3">
        <Avatar name={it.sender} email={it.email ?? ""} class="size-12 shrink-0 self-start text-sm" />
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="min-w-0 flex-1 truncate text-[14px] font-semibold">{it.sender}</span>
            {#if it.importance === 2}<AlertCircle class="size-4 shrink-0 text-red-600" />{/if}
            {#if it.has_attachments}
              <Paperclip class="text-muted-foreground size-3.5 shrink-0" />
              <span class="bg-primary/10 text-primary shrink-0 rounded-full px-2 text-[11px] leading-5 font-medium">{t("alerts.attachment")}</span>
            {/if}
          </div>
          <div class="truncate text-[13px] leading-5 font-semibold">{it.subject || t("mail.no_subject")}</div>
          <div class="text-muted-foreground line-clamp-2 text-[12.5px] leading-[18px]">{it.preview}</div>
        </div>
      </div>
      <!-- Eylem düğmeleri (Ayarlar > Posta > Yeni ileti uyarısı düğmeleri) -->
      {#if buttons.length}
        <div class="mt-2 grid gap-2" style="grid-template-columns:repeat({buttons.length},minmax(0,1fr))">
          {#each buttons as b, i (b + i)}
            {@const Icon = ICONS[b]}
            <button
              class="flex h-9 min-w-0 items-center justify-center gap-1 rounded-lg border px-1.5 text-[12px] whitespace-nowrap {i === 0 ? 'border-primary/30 bg-primary/10 text-primary hover:bg-primary/15' : 'border-black/10 bg-black/[0.03] hover:bg-black/[0.07] dark:border-white/15 dark:bg-white/5 dark:hover:bg-white/10'}"
              title={t(ALERT_ACTIONS[b])}
              onclick={(e) => { e.stopPropagation(); void act(it, b); }}><Icon class="size-3.5 shrink-0" /><span class="truncate">{t(ALERT_ACTIONS[b])}</span></button>
          {/each}
        </div>
      {/if}
      {/if}
    </div>
  {/each}
</div>
