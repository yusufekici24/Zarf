<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { MessageView } from "$lib/types";
  // İkincil pencere kökü: yeni ileti / yanıt ya da açılan ileti. Yedek (önceden
  // yüklenmiş, gizli) pencere olarak açıldıysa içeriği "window://assign" ile alır.
  import { onMount, tick, untrack } from "svelte";
  import { applyScale, watchScreen } from "$lib/zoom";
  import { ModeWatcher, setMode } from "mode-watcher";
  import { listen, emit, emitTo } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { Toaster } from "$lib/components/ui/sonner";
  import DialogHost from "$lib/components/common/DialogHost.svelte";
  import ContactCard from "$lib/components/common/ContactCard.svelte";
  import ComposeWindow from "$lib/components/mail/ComposeWindow.svelte";
  import MessageWindow from "$lib/components/mail/MessageWindow.svelte";
  import SettingsDialog from "$lib/components/dialogs/SettingsDialog.svelte";
  import EventDialog from "$lib/components/calendar/EventDialog.svelte";
  import type { CalEvent, Calendar } from "$lib/types";
  import { api } from "$lib/api";
  import { app, type ComposeRequest } from "$lib/stores/app.svelte";
  import { i18n } from "$lib/i18n.svelte";

  let content = $state<{ kind: string; payload: unknown } | null>(null);
  /** Toplantı/randevu penceresi için takvim listesi */
  let calendars = $state<Calendar[]>([]);
  let eventOpen = $state(true);
  $effect(() => {
    if (content?.kind === "event") {
      eventOpen = true;
      void api.listCalendars().then((c) => (calendars = c)).catch(() => {});
    }
  });
  /** Kaydedilince/silinince ana pencerenin takvimi yenilenir ve pencere kapanır */
  function eventDone() {
    void emit("calendar://changed", {});
    void getCurrentWebviewWindow().destroy();
  }

  function showSettings(p: { tab?: string; from?: string }) {
    if (p.from) app.rulesPrefill = { from: p.from };
    app.settingsDialog = { open: true, tab: p.tab || "general" };
  }
  // İçerik gelip çizilince pencere gösterilir (yedek pencere içeriksizken gizli kalır)
  $effect(() => {
    if (!content || !ready) return;
    const kind = content.kind;
    // (requestAnimationFrame gizli WebKit penceresinde hiç çalışmaz; DOM güncellenince gösterilir)
    // İleti penceresi: ileti ana pencereden hazır gelir; gövde çizilince (en çok 250 ms) gösterilir
      void tick()
        .then(() => (kind === "read" ? contentReady(250) : undefined))
        .then(() =>
        setTimeout(
          () =>
            void api
              .revealWindow()
              // Pencere görününce iki kare beklenir (ilk çizim bitsin), sonra saydamlık kalkar
              .then(() => requestAnimationFrame(() => requestAnimationFrame(() => void api.windowPainted().catch(() => {}))))
              .catch(() => {}),
          0,
        ),
      );
  });

  /** İçerik (ileti gövdesi) hazır olana dek bekler; en çok `ms` */
  function contentReady(ms: number): Promise<void> {
    const w = window as unknown as { __contentReady?: boolean };
    if (w.__contentReady) return Promise.resolve();
    return new Promise((res) => {
      const done = () => {
        clearTimeout(timer);
        window.removeEventListener("mailnix:content-ready", done);
        res();
      };
      const timer = setTimeout(done, ms);
      window.addEventListener("mailnix:content-ready", done);
    });
  }

  // İleti penceresinde yanıtla/ilet: aynı pencere yeni ileti penceresine dönüşür
  $effect(() => {
    const h = (e: Event) => {
      content = { kind: "compose", payload: (e as CustomEvent).detail };
      void invoke("set_window_kind", { kind: "compose" }).catch(() => {});
    };
    window.addEventListener("mailnix:become-compose", h);
    return () => window.removeEventListener("mailnix:become-compose", h);
  });

  // Ayarlar penceresi: içerik gelince sekme açılır
  $effect(() => {
    const c = content;
    if (c?.kind === "settings") untrack(() => showSettings(c.payload as { tab?: string; from?: string }));
  });
  let ready = $state(false);

  // Şifre penceresi: hesap düzenleme açılır, kapatılınca pencere de kapanır
  let accountShown = false;
  $effect(() => {
    const c = content;
    if (c?.kind !== "account") return;
    const open = app.accountDialog.open;
    untrack(() => {
      if (open) accountShown = true;
      else if (accountShown) void getCurrentWebviewWindow().destroy();
      else {
        const account = app.accounts.find((a) => a.id === (c.payload as { id: number }).id) ?? null;
        app.accountDialog = { open: true, account };
      }
    });
  });

  async function prepare() {
    await app.loadAll();
    void applyScale(app.setting("ui_scale", "auto"));
    setMode((app.setting("theme", "system") as "light" | "dark" | "system") || "system");
  }

  onMount(() => watchScreen(() => app.setting("ui_scale", "auto")));
  onMount(() => {
    const unsub: (() => void)[] = [];
    (async () => {
      await i18n.load();
      await prepare();
      ready = true;
      unsub.push(
        // Yalnızca bu pencereye gönderilen içerik (genel "listen" diğer pencerelerinkini de alır)
        await getCurrentWebviewWindow().listen<{ kind: string; payload: unknown }>("window://assign", async (e) => {
          // İçerik hemen gösterilir; yedek pencere beklerken değişmiş olabilecek
          // ayarlar/hesaplar arkadan tazelenir
          (window as unknown as { __contentReady?: boolean }).__contentReady = false;
          content = e.payload;
          void prepare();
        }),
      );
      // Pencere kapatılıp yedeğe alındı: içerik boşaltılır (sonraki açılışta yenisi gelir)
      unsub.push(
        await getCurrentWebviewWindow().listen("window://reset", () => {
          content = null;
          app.accountDialog = { open: false, account: null };
          app.confirmReq = null;
        }),
      );
      // Ayarlar/hesaplar/kategoriler değişince güncel kal
      // (eşitleme arka arkaya bildirim gönderebilir: klasörler kısa beklemeyle bir kez yüklenir)
      let foldersTimer: ReturnType<typeof setTimeout> | undefined;
      unsub.push(
        await listen("mail://changed", () => {
          clearTimeout(foldersTimer);
          foldersTimer = setTimeout(() => void app.loadFolders(), 600);
        }),
      );
      const p = await api.windowReady();
      if (p) {
        (window as unknown as { __contentReady?: boolean }).__contentReady = false;
        content = p;
      }
      // Ayarlar penceresi açıkken başka sekme istenirse
      unsub.push(await getCurrentWebviewWindow().listen<{ tab: string; from?: string }>("settings://show", (e) => showSettings(e.payload)));
      // Başka pencerede ayar/hesap değişince
      unsub.push(await app.listenState(() => {
        void applyScale(app.setting("ui_scale", "auto"));
        setMode((app.setting("theme", "system") as "light" | "dark" | "system") || "system");
        if (app.setting("language", i18n.current) !== i18n.current) void i18n.load();
      }));
    })();

    // Ana penceredeki işlemler (kişi ekle, toplantı oluştur, kaynağı göster) oraya iletilir
    const forward = (name: string) => (e: Event) => {
      void emitTo("main", "app://action", { name, detail: (e as CustomEvent).detail });
      void api.focusMain();
    };
    const names = ["mailnix:add-contact", "mailnix:meeting-from-mail", "mailnix:view-source", "mailnix:save-as", "mailnix:shortcuts"];
    const hs = names.map((n) => [n, forward(n)] as const);
    for (const [n, h] of hs) window.addEventListener(n, h);
    return () => {
      unsub.forEach((u) => u());
      for (const [n, h] of hs) window.removeEventListener(n, h);
    };
  });
</script>

<ModeWatcher defaultMode="system" />
{#if ready && content}
  {#key content}
  {#if content.kind === "compose"}
    <ComposeWindow req={content.payload as ComposeRequest} />
  {:else if content.kind === "read"}
    <MessageWindow id={(content.payload as { id: number }).id} preload={(content.payload as { message?: MessageView }).message} />
  {:else if content.kind === "settings"}
    <SettingsDialog windowed />
    {#await import("$lib/components/dialogs/AccountDialog.svelte") then m}<m.default />{/await}
  {:else if content.kind === "event"}
    {@const ep = content.payload as { event: CalEvent; occurrenceStart?: number | null; meeting?: boolean }}
    {#if calendars.length}
      <EventDialog windowed bind:open={eventOpen} event={ep.event} occurrenceStart={ep.occurrenceStart ?? null} meeting={!!ep.meeting} {calendars} onsaved={eventDone} />
    {/if}
  {:else if content.kind === "account"}
    {#await import("$lib/components/dialogs/AccountDialog.svelte") then m}<m.default />{/await}
  {/if}
  {/key}
{/if}
<DialogHost />
<ContactCard />
<Toaster closeButton position="bottom-right" />
