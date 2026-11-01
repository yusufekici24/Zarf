<script lang="ts">
  // Outlook "Anımsatıcı" penceresi: ayrı, her zaman üstte. Üstte seçili öğenin konusu,
  // başlangıç saati ve konumu; altında zamanı gelen öğeler (Konu / Kalan süre); Çevrimiçi
  // Katıl, Tümünü Kapat, Öğeyi Aç, Kapat; "Yeniden anımsatılmak için Ertele'ye tıklayın".
  import { onMount, tick } from "svelte";
  import { ModeWatcher, setMode } from "mode-watcher";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { emitTo, listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { i18n, t } from "$lib/i18n.svelte";
  import { applyScale } from "$lib/zoom";
  import type { Reminder } from "$lib/types";
  import Bell from "@lucide/svelte/icons/bell";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import CalendarClock from "@lucide/svelte/icons/calendar-clock";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import Video from "@lucide/svelte/icons/video";
  import X from "@lucide/svelte/icons/x";

  let list = $state<Reminder[]>([]);
  let selected = $state<string | null>(null);
  let snooze = $state("");
  let now = $state(Date.now() / 1000);
  let ready = false;
  let known = new Set<string>();

  const current = $derived(list.find((r) => r.key === selected) ?? list[0] ?? null);

  // Outlook erteleme seçenekleri; başlamamış toplantıda "Başlangıçtan … önce" de var
  const DURATIONS = [0, 1, 5, 10, 15, 30, 60, 120, 240, 720, 1440, 2880, 4320, 5760, 10080, 20160];
  function durText(m: number): string {
    if (m < 60) return t("reminder.d_min", m);
    if (m < 720) return t("reminder.d_hour", m / 60);
    if (m === 720) return t("reminder.d_day", "0,5");
    if (m < 10080) return t("reminder.d_day", m / 1440);
    return t("reminder.d_week", m / 10080);
  }
  const options = $derived.by(() => {
    const out: { value: string; label: string }[] = [];
    if (current && current.kind === "event" && current.start > now) {
      for (const m of [15, 10, 5, 0]) if (current.start - m * 60 > now) out.push({ value: `b${m}`, label: t("reminder.before_start", durText(m)) });
    }
    for (const m of DURATIONS.slice(1)) out.push({ value: `m${m}`, label: durText(m) });
    return out;
  });
  // Varsayılan: başlamamış toplantıda "Başlangıçtan 5 dakika önce" (yoksa 5 dakika)
  let snoozeFor = "";
  $effect(() => {
    const opts = options;
    const k = current?.key ?? "";
    // Seçili öğe değişince (ya da seçenek artık yoksa) varsayılana döner
    if (k !== snoozeFor || !opts.some((o) => o.value === snooze)) {
      snoozeFor = k;
      snooze = opts.find((o) => o.value === "b5")?.value ?? opts.find((o) => o.value === "m5")!.value;
    }
  });

  function due(r: Reminder): string {
    const diff = Math.round((r.start - now) / 60);
    if (Math.abs(diff) < 1) return t("reminder.now");
    const a = Math.abs(diff);
    const v = a < 60 ? t(diff > 0 ? "reminder.in_minutes" : "reminder.overdue_minutes", a) : a < 1440 ? t(diff > 0 ? "reminder.in_hours" : "reminder.overdue_hours", Math.round(a / 60)) : t(diff > 0 ? "reminder.in_days" : "reminder.overdue_days", Math.round(a / 1440));
    return v;
  }
  function when(r: Reminder): string {
    const d = new Date(r.start * 1000);
    const date = d.toLocaleDateString(i18n.locale, { weekday: "long", day: "numeric", month: "long", year: "numeric" });
    if (r.all_day) return `${date} (${t("reminder.all_day")})`;
    return `${date} ${d.toLocaleTimeString(i18n.locale, { hour: "2-digit", minute: "2-digit" })}`;
  }

  /** Yeni anımsatıcı gelince Outlook gibi kısa bir zil sesi */
  function chime() {
    if (app.setting("reminder_sound", "1") !== "1") return;
    try {
      const ctx = new AudioContext();
      [880, 1175].forEach((f, i) => {
        const o = ctx.createOscillator();
        const g = ctx.createGain();
        o.type = "sine";
        o.frequency.value = f;
        const t0 = ctx.currentTime + i * 0.18;
        g.gain.setValueAtTime(0.0001, t0);
        g.gain.exponentialRampToValueAtTime(0.18, t0 + 0.02);
        g.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.5);
        o.connect(g).connect(ctx.destination);
        o.start(t0);
        o.stop(t0 + 0.55);
      });
      setTimeout(() => void ctx.close(), 1500);
    } catch {
      /* ses yok */
    }
  }

  async function reload() {
    const next = await api.getReminders().catch(() => [] as Reminder[]);
    const fresh = next.some((r) => !known.has(r.key));
    known = new Set(next.map((r) => r.key));
    list = next;
    if (!list.some((r) => r.key === selected)) selected = list[0]?.key ?? null;
    void getCurrentWindow().setTitle(t("reminder.title", list.length)).catch(() => {});
    if (!list.length) {
      hideWin();
      return;
    }
    if (fresh) chime();
  }

  /** Değişiklik bildirimi: diğer pencereler (uyarı kartları) güncellensin */
  async function changed(keys: string[]) {
    list = list.filter((r) => !keys.includes(r.key));
    if (!list.some((r) => r.key === selected)) selected = list[0]?.key ?? null;
    await emitTo("alerts", "alerts://reminders-done", keys).catch(() => {});
    void getCurrentWindow().setTitle(t("reminder.title", list.length)).catch(() => {});
    if (!list.length) hideWin();
  }

  async function dismiss(keys: string[]) {
    for (const k of keys) await api.dismissReminder(k).catch(() => {});
    await changed(keys);
  }

  async function doSnooze() {
    const r = current;
    if (!r) return;
    const v = snooze;
    const m = Number(v.slice(1));
    if (v.startsWith("b")) await api.dismissReminder(r.key, null, r.start - m * 60).catch(() => {});
    else await api.dismissReminder(r.key, m).catch(() => {});
    await changed([r.key]);
  }

  async function openItem(r: Reminder | null) {
    if (!r) return;
    await emitTo("main", "app://open-item", { module: r.kind === "event" ? "calendar" : "tasks", id: r.item_id }).catch(() => {});
    await api.focusMain().catch(() => {});
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") hideWin();
    else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement) && !(e.target instanceof HTMLSelectElement)) void openItem(current);
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const i = list.findIndex((r) => r.key === current?.key);
      const j = Math.max(0, Math.min(list.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      if (list[j]) selected = list[j].key;
      e.preventDefault();
    }
  }

  /** Pencereyi gizler (uygulama komutu: pencere izinlerinden bağımsız) */
  function hideWin() {
    void invoke("close_reminders").catch(() => void getCurrentWindow().hide().catch(() => {}));
  }

  /** Pencere gösterilince (saydam) iki kare sonra görünür yapılır */
  function painted() {
    requestAnimationFrame(() => requestAnimationFrame(() => void api.windowPainted().catch(() => {})));
  }

  onMount(() => {
    const offs: (() => void)[] = [];
    const clock = setInterval(() => (now = Date.now() / 1000), 15000);
    (async () => {
      await i18n.load();
      await app.loadAll().catch(() => {});
      setMode((app.setting("theme", "system") as "light" | "dark" | "system") || "system");
      await applyScale(app.setting("ui_scale", "auto"));
      await reload();
      ready = true;
      await tick();
      // İlk açılış: içerik çizilince gösterilir (beyaz pencere görünmez)
      void api.revealWindow().then(painted).catch(() => {});
      offs.push(
        await listen("reminders://refresh", async () => {
          now = Date.now() / 1000;
          await reload();
          painted();
        }),
      );
      offs.push(await listen<string[]>("reminders://done", (e) => void changed(e.payload)));
      offs.push(await app.listenState(() => setMode((app.setting("theme", "system") as "light" | "dark" | "system") || "system")));
    })();
    return () => {
      clearInterval(clock);
      offs.forEach((f) => f());
    };
  });
  void ready;

  const btn = "bg-background hover:bg-accent active:bg-accent/70 h-8 rounded-[4px] border border-[#8a8a8a] px-3 text-[13px] shadow-[0_1px_2px_rgba(0,0,0,0.06)] disabled:opacity-40 disabled:pointer-events-none dark:border-white/30";
</script>

<svelte:window onkeydown={onkey} />
<ModeWatcher defaultMode="system" />

<div class="bg-chrome flex h-screen flex-col overflow-hidden text-[13px] select-none">
  <!-- Başlık çubuğu -->
  <header data-tauri-drag-region class="flex h-9 shrink-0 items-center gap-2 pl-3">
    <Bell class="pointer-events-none size-4 text-amber-500" />
    <span data-tauri-drag-region class="min-w-0 flex-1 truncate">{t("reminder.title", list.length)}</span>
    <button class="flex h-full w-11 items-center justify-center hover:bg-[#c42b1c] hover:text-white" title={t("common.close")} aria-label={t("common.close")} onclick={() => hideWin()}><X class="size-4" /></button>
  </header>

  <div class="bg-background mx-2 mb-2 flex min-h-0 flex-1 flex-col gap-3 rounded-[4px] border p-3 shadow-sm">
    <!-- Seçili öğe -->
    {#if current}
      <div class="flex gap-3">
        <div class="bg-primary/10 text-primary flex size-10 shrink-0 items-center justify-center rounded-[4px]">
          {#if current.kind === "event"}<CalendarClock class="size-6" />{:else}<ListTodo class="size-6" />{/if}
        </div>
        <div class="min-w-0 flex-1">
          <div class="truncate text-[15px] font-semibold">{current.title || t("mail.no_subject")}</div>
          <div class="text-muted-foreground truncate">{current.kind === "event" ? t("reminder.start_time", when(current)) : t("reminder.due_date", when(current))}</div>
          {#if current.location}<div class="text-muted-foreground truncate">{t("reminder.location", current.location)}</div>{/if}
          {#if current.organizer}<div class="text-muted-foreground truncate">{t("reminder.organizer", current.organizer)}</div>{/if}
        </div>
      </div>
    {/if}

    <!-- Liste: Konu / Kalan süre -->
    <div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[4px] border">
      <div class="bg-muted/50 text-muted-foreground grid shrink-0 grid-cols-[1fr_150px] border-b px-2 py-1 text-[12px] font-semibold">
        <span>{t("reminder.col_subject")}</span><span>{t("reminder.col_due")}</span>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto" role="listbox" tabindex="0">
        {#each list as r (r.key)}
          <button
            role="option"
            aria-selected={current?.key === r.key}
            class="grid w-full grid-cols-[1fr_150px] items-center px-2 py-1 text-left {current?.key === r.key ? 'bg-primary/15' : 'hover:bg-accent'}"
            onclick={() => (selected = r.key)}
            ondblclick={() => void openItem(r)}
          >
            <span class="flex min-w-0 items-center gap-2">
              {#if r.kind === "event"}<CalendarIcon class="text-primary size-4 shrink-0" />{:else}<ListTodo class="size-4 shrink-0 text-emerald-600" />{/if}
              <span class="truncate">{r.title || t("mail.no_subject")}</span>
            </span>
            <span class="truncate {r.start < now - 30 ? 'font-semibold text-red-600' : ''}">{due(r)}</span>
          </button>
        {/each}
      </div>
    </div>

    <!-- Düğmeler -->
    <div class="flex shrink-0 items-center gap-2">
      {#if current?.join_url}
        <button class="flex h-8 items-center gap-1.5 rounded-[4px] border border-[#5b5fc7] bg-[#5b5fc7] px-3 text-[13px] text-white shadow-[0_1px_2px_rgba(0,0,0,0.06)] hover:bg-[#4f52b2]" onclick={() => current && void api.openUrl(current.join_url)}><Video class="size-4" />{t("reminder.join")}</button>
      {/if}
      <span class="flex-1"></span>
      <button class={btn} disabled={!list.length} onclick={() => void dismiss(list.map((r) => r.key))}>{t("reminder.dismiss_all_btn")}</button>
      <button class={btn} disabled={!current} onclick={() => void openItem(current)}>{t("reminder.open_item")}</button>
      <button class={btn} disabled={!current} onclick={() => current && void dismiss([current.key])}>{t("reminder.close")}</button>
    </div>

    <!-- Ertele -->
    <div class="shrink-0">
      <div class="text-muted-foreground mb-1">{t("reminder.snooze_hint")}</div>
      <div class="flex items-center gap-2">
        <select class="border-input bg-background h-8 min-w-0 flex-1 rounded-[4px] border px-2 text-[13px]" bind:value={snooze}>
          {#each options as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
        </select>
        <button class={btn} disabled={!current} onclick={() => void doSnooze()}>{t("reminder.snooze")}</button>
      </div>
    </div>
  </div>
</div>
