<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { applyScale, stepScale, fitWindow, watchScreen } from "$lib/zoom";
  import { ModeWatcher, setMode } from "mode-watcher";
  import { listen } from "@tauri-apps/api/event";
  import { open as openDialog, save as saveDialog } from "$lib/dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Toaster } from "$lib/components/ui/sonner";
  import { Button } from "$lib/components/ui/button";
  import { toast } from "svelte-sonner";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { mail } from "$lib/stores/mail.svelte";
  import { i18n, t } from "$lib/i18n.svelte";
  import type { MessageView, ModuleName } from "$lib/types";
  import MailView from "$lib/components/mail/MailView.svelte";
  import { openFile } from "$lib/actions";
  import { IS_MAIN, WINDOW_LABEL } from "$lib/window";
  import { windowFocus } from "$lib/stores/focus.svelte";
  import { fmtDateTime } from "$lib/format";
  // Açılışta gerekmeyen bölümler ayrı parçalar halinde yüklenir: ana pencere ve ileti
  // pencereleri daha az kod ayrıştırarak daha hızlı açılır
  const views = {
    calendar: () => import("$lib/components/calendar/CalendarView.svelte"),
    contacts: () => import("$lib/components/contacts/ContactsView.svelte"),
    tasks: () => import("$lib/components/tasks/TasksView.svelte"),
    notes: () => import("$lib/components/notes/NotesView.svelte"),
  };
  const lazyAccountDialog = () => import("$lib/components/dialogs/AccountDialog.svelte");

  // Ayarlar ayrı pencerede açılır (Outlook Seçenekleri gibi): ana penceredeki istekler oraya
  $effect(() => {
    if (!IS_MAIN || !app.settingsDialog.open) return;
    const { tab } = app.settingsDialog;
    untrack(() => {
      const from = app.rulesPrefill?.from;
      app.rulesPrefill = null;
      app.settingsDialog = { open: false, tab };
      app.openSettings(tab, from);
    });
  });
  import DialogHost from "$lib/components/common/DialogHost.svelte";
  import ContactCard from "$lib/components/common/ContactCard.svelte";
  import SearchBox from "$lib/components/common/SearchBox.svelte";
  import Wifi from "@lucide/svelte/icons/wifi";
  import WifiOff from "@lucide/svelte/icons/wifi-off";
  import MailIcon from "@lucide/svelte/icons/mail";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import Users from "@lucide/svelte/icons/users";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import Search from "@lucide/svelte/icons/search";
  import Settings from "@lucide/svelte/icons/settings";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import X from "@lucide/svelte/icons/x";
  import Menu from "@lucide/svelte/icons/menu";
  import HelpCircle from "@lucide/svelte/icons/circle-help";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import Copy from "@lucide/svelte/icons/copy";
  import UserPlus from "@lucide/svelte/icons/user-plus";

  let ready = $state(false);
  let sourceText = $state<string | null>(null);
  let shortcutsOpen = $state(false);
  let searchInput: HTMLInputElement | undefined = $state();

  const MODULES: { key: ModuleName; icon: typeof MailIcon; label: string }[] = [
    { key: "mail", icon: MailIcon, label: "module.mail" },
    { key: "calendar", icon: CalendarIcon, label: "module.calendar" },
    { key: "contacts", icon: Users, label: "module.contacts" },
    { key: "tasks", icon: ListTodo, label: "module.tasks" },
    { key: "notes", icon: StickyNote, label: "module.notes" },
  ];

  // Posta kutusu doluluğu: açılışta ve 15 dakikada bir
  let usage = $state<{ used: number; quota: number } | null>(null);
  const fmtSize = (b: number) => (b >= 1024 ** 3 ? `${(b / 1024 ** 3).toFixed(1)} GB` : `${Math.round(b / 1024 ** 2)} MB`);
  $effect(() => {
    if (!app.accounts.some((a) => a.kind === "exchange")) return;
    const load = () => void api.mailboxUsage().then((u) => (usage = u)).catch(() => {});
    const first = setTimeout(load, 4000);
    const timer = setInterval(load, 15 * 60 * 1000);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  });

  function setModule(m: ModuleName) {
    app.module = m;
    app.search = "";
  }

  /** Önceki oturum çöktüyse yarım kalan iletiler geri sunulur; "İptal" kayıtları sonraki açılışa bırakır */
  async function offerRecovery() {
    const items = (await api.listRecovery().catch(() => [] as string[])).flatMap((s) => {
      try {
        return [JSON.parse(s)];
      } catch {
        return [];
      }
    });
    if (!items.length) return;
    const r = await app.choose(t("recovery.title"), t("recovery.text", items.length), t("recovery.restore"), t("recovery.discard"));
    if (r === "cancel") return;
    for (const it of items) {
      if (r === "ok") app.compose({ mode: "new", data: it, attachments: it.attachments ?? [], recoveryId: it.id });
      else void api.removeRecovery(it.id).catch(() => {});
    }
  }

  async function notify(title: string, body: string) {
    try {
      await api.notify(title, body, app.setting("notify_sound", "1") === "1");
    } catch {
      /* bildirim hizmeti yoksa sessizce geç */
    }
  }

  async function saveAs(id: number) {
    const m = await api.getMessage(id);
    const base = (m.subject || "ileti").replace(/[\\/:*?"<>|]/g, "_").slice(0, 100);
    const p = await saveDialog({
      defaultPath: `${base}.msg`,
      filters: [
        { name: t("filter.outlook_msg"), extensions: ["msg"] },
        { name: t("filter.eml"), extensions: ["eml"] },
        { name: "HTML", extensions: ["html"] },
        { name: t("filter.text"), extensions: ["txt"] },
      ],
    });
    if (p) {
      try {
        await api.saveMessageAs(id, p);
        toast.success(t("common.saved"));
      } catch (e) {
        app.error(e);
      }
    }
  }

  async function meetingFromMail(id: number) {
    const m = await api.getMessage(id);
    const me = app.accounts.map((a) => a.email.toLowerCase());
    const addrs = [m.sender, m.to_addr, m.cc].join(",").match(/[\w.+-]+@[\w.-]+/g) ?? [];
    const attendees = [...new Set(addrs.map((a) => a.toLowerCase()))].filter((a) => !me.includes(a)).join(",");
    app.newItem("calendar", { title: m.subject, description: m.text.slice(0, 4000), attendees });
  }

  function onkeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    const typing = target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    // Arayüz ölçeği: Ctrl + / Ctrl - / Ctrl 0 (Outlook yakınlaştırma gibi)
    if (ctrl && (e.key === "+" || e.key === "=" || e.key === "-" || e.key === "0") && !e.altKey) {
      e.preventDefault();
      const cur = app.setting("ui_scale", "auto");
      void app.setSetting("ui_scale", e.key === "0" ? "auto" : stepScale(cur, e.key === "-" ? -1 : 1));
      return;
    }
    if (e.key === "F9") {
      e.preventDefault();
      void api.syncAll();
      return;
    }
    if (ctrl && k === "e") {
      e.preventDefault();
      searchInput?.focus();
      return;
    }
    if (ctrl && !e.shiftKey && ["1", "2", "3", "4", "5"].includes(e.key)) {
      e.preventDefault();
      setModule(MODULES[Number(e.key) - 1].key);
      return;
    }
    if (ctrl && e.shiftKey) {
      const map: Record<string, () => void> = {
        m: () => app.compose(),
        a: () => app.newItem("calendar"),
        q: () => app.newItem("calendar", { attendees: "" }),
        c: () => app.newItem("contacts"),
        k: () => app.newItem("tasks"),
        n: () => app.newItem("notes"),
        r: () => mail.reply("reply_all"),
      };
      if (map[k] && !(typing && k !== "m")) {
        e.preventDefault();
        map[k]();
        return;
      }
    }
    if (ctrl && k === "n" && !e.shiftKey) {
      e.preventDefault();
      app.newItem(app.module);
      return;
    }
    if (ctrl && k === "o") {
      e.preventDefault();
      void openFile();
      return;
    }
    // Ctrl+C: metin seçili değilse seçili iletiler kopyalanır
    if (ctrl && k === "c" && !e.shiftKey && !typing && app.module === "mail" && !window.getSelection()?.toString()) {
      window.dispatchEvent(new Event("mailnix:copy-items"));
      return;
    }
    if (e.key === "F1") {
      e.preventDefault();
      shortcutsOpen = true;
      return;
    }
    if (typing || app.module !== "mail") return;
    if (ctrl && k === "r") { e.preventDefault(); mail.reply("reply"); }
    else if (ctrl && k === "f") { e.preventDefault(); mail.reply("forward"); }
    else if (ctrl && k === "q") { e.preventDefault(); void mail.setRead(mail.ids(), true); }
    else if (ctrl && k === "u") { e.preventDefault(); void mail.setRead(mail.ids(), false); }
    else if (ctrl && k === "p") { e.preventDefault(); window.dispatchEvent(new Event("mailnix:print")); }
    else if (e.key === "Delete") { e.preventDefault(); void mail.delete(e.shiftKey); }
    else if (e.key === "Backspace") { e.preventDefault(); void mail.moveToRole("archive"); }
    else if (e.key === "Insert") { e.preventDefault(); void mail.toggleFlag(); }
    else if (ctrl && e.altKey && k === "j") { e.preventDefault(); void mail.moveToRole("junk"); }
  }

  // Pencere büyütülmüş mü (büyüt/eski boyut düğmesi için)
  let maximized = $state(false);
  // Ekrana göre yazı/simge ölçeği (Ayarlar > Genel ya da Ctrl +/-)
  $effect(() => {
    void applyScale(app.setting("ui_scale", "auto"));
  });
  // Çözünürlük değişince ölçek ve pencere boyutu yeniden (açılıştaki gibi) ayarlanır
  // (ikincil pencereler kendi izleyicisini kurar)
  onMount(() => (IS_MAIN ? watchScreen(() => app.setting("ui_scale", "auto")) : undefined));

  /**
   * İlk açılışta: varsayılan e-posta uygulaması olsun mu (e-posta bağlantıları ve .msg/.eml
   * dosyaları MailNix ile açılır). Hayır denirse Ayarlar'da nereden yapılacağı söylenir.
   * Bir kez sorulur; hesap kurulumu açıksa o kapanınca sorulur.
   */
  async function askDefaultApp() {
    if (app.setting("default_app_asked", "0") === "1") return;
    if (await api.defaultAppStatus().catch(() => true)) {
      void app.setSetting("default_app_asked", "1");
      return;
    }
    while (app.accountDialog.open) await new Promise((r) => setTimeout(r, 800));
    void app.setSetting("default_app_asked", "1");
    const yes = await app.confirm(t("default_app.title"), t("default_app.text"), { ok: t("default_app.yes"), cancel: t("default_app.no") });
    if (yes) {
      try {
        await api.setDefaultApp();
        toast.success(t("default_app.done"));
      } catch (e) {
        app.error(e);
      }
    } else {
      toast.info(t("default_app.later"), { duration: 15000 });
    }
  }

  onMount(() => {
    if (!IS_MAIN) return;
    windowFocus.start();
    const w = getCurrentWindow();
    const refresh = () => w.isMaximized().then((m) => (maximized = m)).catch(() => {});
    refresh();
    let off: (() => void) | undefined;
    w.onResized(refresh).then((f) => (off = f)).catch(() => {});
    return () => off?.();
  });

  onMount(() => {
    if (!IS_MAIN) return;
    const unsub: (() => void)[] = [];
    (async () => {
      await i18n.load();
      await app.loadAll();
      // Açılışta pencere ekrana ve ölçeğe göre boyutlanır (pencere henüz gizli)
      await applyScale(app.setting("ui_scale", "auto"));
      await fitWindow();
      setMode((app.setting("theme", "system") as "light" | "dark" | "system") || "system");
      app.module = (app.setting("start_module", "mail") as ModuleName) || "mail";
      ready = true;
      // İlk çizim bitince pencere gösterilir: beyaz ekran ve yerleşim kayması görünmez
      // (requestAnimationFrame gizli WebKit penceresinde hiç çalışmaz; DOM güncellenince gösterilir)
      void tick().then(() => setTimeout(() => void api.revealWindow().catch(() => {}), 0));
      // Diğer bölümler açılış bittikten sonra boşta yüklenir (geçiş anında beklenmez)
      setTimeout(() => {
        for (const f of [...Object.values(views), lazyAccountDialog]) void f().catch(() => {});
      }, 4000);

      // Eşitleme sırasında art arda gelen değişiklik olayları birleştirilir
      let changedTimer: ReturnType<typeof setTimeout> | null = null;
      let changedAll = false;
      const changedFolders = new Set<number>();
      unsub.push(await listen<{ folder: number | null }>("mail://changed", (e) => {
        if (e.payload?.folder == null) changedAll = true;
        else changedFolders.add(e.payload.folder);
        if (changedTimer) return;
        changedTimer = setTimeout(async () => {
          changedTimer = null;
          const all = changedAll;
          const touched = new Set(changedFolders);
          changedAll = false;
          changedFolders.clear();
          await app.loadFolders();
          if (all || mail.folderId == null || touched.has(mail.folderId) || mail.searchFolder || app.search) app.bump("mail");
        }, 250);
      }));
      unsub.push(await listen<boolean>("mail://bg", (e) => (app.bgSync = e.payload)));
      unsub.push(await listen<string>("mail://status", (e) => (app.status = e.payload)));
      // Bağlantı durumu: Outlook gibi bir kez bildirilir, sonra durum çubuğunda gösterilir
      app.net = (await api.netState().catch(() => null)) || "online";
      unsub.push(await listen<"online" | "disconnected" | "work_offline">("net://state", (e) => {
        const prev = app.net;
        app.net = e.payload;
        if (e.payload === "disconnected" && prev === "online") toast.warning(t("net.went_offline"), { id: "net", duration: 8000 });
        else if (e.payload === "online" && prev === "disconnected") toast.success(t("net.back_online"), { id: "net", duration: 3000 });
      }));
      let pausedAt = 0;
      unsub.push(await listen<string>("mail://error", (e) => {
        // Hesap duraklatma bildirimi ayrıca (eylem düğmesiyle) gösterilir
        if (Date.now() - pausedAt < 4000) return;
        toast.error(e.payload, { duration: 8000 });
      }));
      unsub.push(await listen<{ id: number; email: string; error: string }>("account://auth_failed", async (e) => {
        pausedAt = Date.now();
        await app.loadAccounts();
        const account = app.accounts.find((a) => a.id === e.payload.id) ?? null;
        // Yalnızca ileti penceresi açıkken (ana pencere hiç gösterilmedi) şifre ayrı pencerede sorulur
        if (await api.mainHidden().catch(() => false)) {
          void api.openWindow("account", { id: e.payload.id }, t("account.paused_title", e.payload.email)).catch(() => {});
          return;
        }
        toast.error(t("account.paused_title", e.payload.email), {
          description: t("account.paused_text"),
          duration: Infinity,
          action: { label: t("account.update_password"), onClick: () => (app.accountDialog = { open: true, account }) },
        });
        if (!(await getCurrentWindow().isFocused().catch(() => false))) void notify(t("account.paused_title", e.payload.email), t("account.paused_text"));
      }));
      unsub.push(await listen("calendar://changed", () => app.bump("calendar")));
      unsub.push(await listen<{ folder: number; more: boolean }>("mail://older", (e) => mail.onOlder(e.payload.folder, e.payload.more)));
      unsub.push(await listen("contacts://changed", () => app.bump("contacts")));
      unsub.push(await listen<string>("mail://sent", (e) => toast.success(t("mail.sent_toast", e.payload || t("mail.no_subject")))));
      // Outlook masaüstü uyarısı gibi: yeni postada her zaman sistem bildirimi
      // (Windows'ta sağ alt köşe, Linux'ta bildirim alanı); pencere küçültülmüş olsa da
      unsub.push(await listen<{ id: number; subject: string; sender: string }[]>("mail://new", async (e) => {
        if (app.setting("notify_new_mail", "1") !== "1") return;
        // Uygulama önde açık, arkada ya da simge durumunda olsun: her yeni postada uyarı gelir
        // Uyarı kartından "Sessize al" seçildiyse bir saat uyarı yok
        if (Number(app.setting("alerts_muted_until", "0")) > Date.now() / 1000) return;
        const list = e.payload;
        // Outlook/Skype gibi sağ altta üst üste dizilen kartlar (ya da sistem bildirimi)
        if (app.setting("notify_style", "alerts") === "alerts") {
          void api.showMailAlerts(list).catch(() => {});
          return;
        }
        if (list.length === 1) void notify(list[0].sender, list[0].subject || t("mail.no_subject"));
        else void notify(t("mail.new_n", list.length), list.slice(0, 3).map((m) => `${m.sender}: ${m.subject || t("mail.no_subject")}`).join("\n"));
      }));
      // Anımsatıcı penceresinden / toplantı kartından "Öğeyi Aç"
      unsub.push(await listen<{ module: ModuleName; id: number }>("app://open-item", (e) => app.openItem(e.payload.module, e.payload.id)));
      unsub.push(await listen<{ id: number }>("app://open-message", (e) => app.openMessage(e.payload.id)));
      unsub.push(await listen<number>("app://calendar-goto", (e) => {
        app.calendarGoto = e.payload;
        app.module = "calendar";
      }));
      unsub.push(await listen("app://refresh-mail", () => void mail.load(true)));
      unsub.push(await listen<string[]>("app://open-args", (e) => e.payload.forEach((p) => app.openPath(p))));
      // İkincil pencerelerden gelen istekler
      unsub.push(await listen<{ tab: string }>("app://open-settings", (e) => (app.openSettings(e.payload.tab))));
      unsub.push(await listen("app://open-account", () => (app.accountDialog = { open: true, account: null })));
      unsub.push(await listen<{ from: string }>("app://open-rules", (e) => app.openRulesFor(e.payload.from)));
      // Ayarlar penceresinde (ya da başka pencerede) yapılan değişiklikler
      unsub.push(await app.listenState(async () => {
        void applyScale(app.setting("ui_scale", "auto"));
        setMode((app.setting("theme", "system") as "light" | "dark" | "system") || "system");
        if (app.setting("language", i18n.current) !== i18n.current) await i18n.load();
        app.bump("mail");
      }));
      unsub.push(await listen<{ name: string; detail: unknown }>("app://action", (e) => window.dispatchEvent(new CustomEvent(e.payload.name, { detail: e.payload.detail }))));
      // Yeni ileti penceresinden gönderildi: Outlook'taki "Gönderimi geri al" burada
      unsub.push(await listen<{ id: number; undo: number; sendAt: number | null; subject: string }>("compose://sent", (e) => {
        const { id, undo, sendAt } = e.payload;
        if (app.net !== "online" && !sendAt) toast(t("mail.queued_offline"), { duration: 6000 });
        if (undo > 0) {
          let undone = false;
          toast(t("compose.sending"), {
            duration: undo * 1000,
            action: {
              label: t("compose.undo_send"),
              onClick: async () => {
                undone = true;
                try {
                  const draft = await api.cancelOutbox(id);
                  app.compose({ mode: "draft", sourceId: draft });
                } catch (err) {
                  app.error(err);
                }
              },
            },
            onAutoClose: () => { if (!undone) void api.retryOutbox(id); },
            onDismiss: () => { if (!undone) void api.retryOutbox(id); },
          });
        } else if (sendAt) toast.success(t("compose.scheduled", fmtDateTime(sendAt)));
      }));

      const args = await api.startupArgs();
      for (const p of args) await app.openPath(p);

      if (!args.length && !app.accounts.length && app.setting("welcome_shown", "0") !== "1") {
        void app.setSetting("welcome_shown", "1");
        // İlk açılışta otomatik hesap kurulumu
        app.accountDialog = { open: true, account: null, auto: true };
      }
      void askDefaultApp();
      void offerRecovery();
    })();

    const handlers: [string, (e: Event) => void][] = [
      ["mailnix:shortcuts", () => (shortcutsOpen = true)],
      ["mailnix:save-as", (e) => saveAs(Number((e as CustomEvent).detail))],
      ["mailnix:meeting-from-mail", (e) => meetingFromMail(Number((e as CustomEvent).detail))],
      ["mailnix:add-contact", (e) => {
        if (app.module === "contacts") return; // kişiler görünümü kendisi işler
        const raw = String((e as CustomEvent).detail ?? "");
        const m = /^\s*"?([^"<]*?)"?\s*<([^>]+)>\s*$/.exec(raw);
        const name = (m ? m[1] : raw.includes("@") ? "" : raw).trim();
        const email = (m ? m[2] : raw.includes("@") ? raw : "").trim();
        const parts = name.split(/\s+/).filter(Boolean);
        app.newItem("contacts", { first_name: parts.length > 1 ? parts.slice(0, -1).join(" ") : parts[0] ?? "", last_name: parts.length > 1 ? parts[parts.length - 1] : "", display_name: name, email });
      }],
      ["mailnix:view-source", async (e) => {
        const id = Number((e as CustomEvent).detail);
        const tmp = `${await import("@tauri-apps/api/path").then((p) => p.tempDir())}/mailnix-src-${id}.eml`;
        await api.saveMessageAs(id, tmp);
        sourceText = await import("@tauri-apps/api/core").then((c) => c.invoke<string>("read_text_file", { path: tmp }));
      }],
    ];
    for (const [n, h] of handlers) window.addEventListener(n, h);
    return () => {
      unsub.forEach((u) => u());
      for (const [n, h] of handlers) window.removeEventListener(n, h);
    };
  });
</script>

<svelte:window onkeydown={(e) => IS_MAIN && onkeydown(e)} />
{#if IS_MAIN}<ModeWatcher defaultMode="system" />{/if}

{#if WINDOW_LABEL === "alerts"}
  {#await import("$lib/components/common/AlertsWindow.svelte") then m}<m.default />{/await}
{:else if WINDOW_LABEL === "reminders"}
  {#await import("$lib/components/common/RemindersWindow.svelte") then m}<m.default />{/await}
{:else if !IS_MAIN}
  {#await import("$lib/components/SecondaryWindow.svelte") then m}<m.default />{/await}
{:else if !ready}
  <div class="flex h-screen items-center justify-center">
    <img src="/icon.svg" alt="" class="size-20 animate-pulse" />
  </div>
{:else}
  <div class="bg-chrome flex h-screen flex-col border {maximized ? 'border-transparent' : windowFocus.focused ? 'border-primary' : 'border-black/25 dark:border-white/20'}">
    <!-- Üst çubuk -->
    <!-- Üst çubuk aynı zamanda pencere başlığıdır: boş alandan sürüklenir, çift tıklayınca büyür -->
    <!-- Outlook başlık çubuğu: uygulama simgesi, hızlı erişim, ortada arama, pencere düğmeleri -->
    <header data-tauri-drag-region class="bg-chrome flex h-10 shrink-0 items-center gap-1 pl-2 select-none">
      <img src="/icon.svg" alt="" class="pointer-events-none mx-1 size-5" />
      <button class="hover:bg-black/5 dark:hover:bg-white/10 rounded p-1.5" title={t("mail.send_receive") + " (F9)"} onclick={() => api.syncAll()}><RefreshCw class="text-primary size-4 {app.status ? 'animate-spin' : ''}" /></button>
      <div data-tauri-drag-region class="h-full min-w-4 flex-1"></div>
      <SearchBox bind:input={searchInput} />
      <div data-tauri-drag-region class="h-full min-w-4 flex-1"></div>
      {#if app.status}<span data-tauri-drag-region class="text-muted-foreground hidden max-w-60 truncate text-xs lg:inline">{app.status}</span>{/if}
      <button class="hover:bg-black/5 dark:hover:bg-white/10 rounded p-1.5" title={t("help.shortcuts") + " (F1)"} onclick={() => (shortcutsOpen = true)}><HelpCircle class="size-4" /></button>
      <button class="hover:bg-black/5 dark:hover:bg-white/10 rounded p-1.5" title={t("settings.title")} onclick={() => app.openSettings("general")}><Settings class="size-4" /></button>
      <!-- Pencere düğmeleri -->
      <div class="ml-1 flex h-full items-stretch">
        <button class="flex w-11 items-center justify-center hover:bg-black/10 dark:hover:bg-white/10" title={t("window.minimize")} aria-label={t("window.minimize")} onclick={() => getCurrentWindow().minimize()}><Minus class="size-4" /></button>
        <button class="flex w-11 items-center justify-center hover:bg-black/10 dark:hover:bg-white/10" title={maximized ? t("window.restore") : t("window.maximize")} aria-label={maximized ? t("window.restore") : t("window.maximize")} onclick={() => getCurrentWindow().toggleMaximize()}>
          {#if maximized}<Copy class="size-3.5 -scale-x-100" />{:else}<Square class="size-3.5" />{/if}
        </button>
        <button class="flex w-11 items-center justify-center hover:bg-[#c42b1c] hover:text-white" title={t("common.close")} aria-label={t("common.close")} onclick={() => getCurrentWindow().close()}><X class="size-4" /></button>
      </div>
    </header>

    {#if !app.accounts.length}
      <div class="mx-2 mb-1 flex items-center gap-3 rounded-md bg-amber-50 px-4 py-1.5 text-sm text-amber-900 dark:bg-amber-950 dark:text-amber-100">
        <UserPlus class="size-4" />
        <span class="flex-1">{t("welcome.banner")}</span>
        <Button size="sm" onclick={() => (app.accountDialog = { open: true, account: null })}>{t("account.add")}</Button>
      </div>
    {/if}

    <div class="bg-chrome relative flex min-h-0 flex-1">
      {#if app.backstage}
        {#await import("$lib/components/ribbon/MainBackstage.svelte") then m}<m.default />{/await}
      {/if}
      <!-- Gezinti rayı (Outlook: sol dikey modül çubuğu) -->
      <nav class="flex w-12 shrink-0 flex-col items-center gap-1.5 py-1">
        {#each MODULES as m, i (m.key)}
          <button class="nav-rail-btn relative" data-active={app.module === m.key} title="{t(m.label)} (Ctrl+{i + 1})" onclick={() => setModule(m.key)}>
            <m.icon class="size-5" />
            {#if m.key === "mail" && app.unreadTotal}
              <span class="absolute -top-0.5 -right-1 flex h-[15px] min-w-[15px] items-center justify-center rounded-full bg-[#d13438] px-[3px] text-[9px] leading-none font-bold text-white tabular-nums ring-2 ring-[var(--chrome,#f0f0f0)]">{app.unreadTotal > 99 ? "99+" : app.unreadTotal}</span>
            {/if}
          </button>
        {/each}
        <!-- Posta kutusu doluluğu (Outlook gibi): yüzde halka içinde -->
        {#if usage && usage.quota > 0}
          {@const pct = Math.min(100, Math.round((usage.used / usage.quota) * 100))}
          {@const C = 2 * Math.PI * 13}
          <div class="relative mt-auto mb-0.5 size-8" title={t("status.mailbox_usage", fmtSize(usage.used), fmtSize(usage.quota), pct)}>
            <svg viewBox="0 0 32 32" class="size-8 -rotate-90">
              <circle cx="16" cy="16" r="13" fill="none" stroke="currentColor" stroke-width="3" class="text-black/10 dark:text-white/15" />
              <circle cx="16" cy="16" r="13" fill="none" stroke-width="3" stroke-linecap="round" stroke-dasharray="{(C * pct) / 100} {C}" class={pct >= 95 ? "stroke-[#d13438]" : pct >= 85 ? "stroke-amber-500" : "stroke-primary"} />
            </svg>
            <span class="absolute inset-0 flex items-center justify-center text-[9px] font-semibold tabular-nums">{pct}%</span>
          </div>
        {/if}
      </nav>
      <main class="min-w-0 flex-1 pr-1.5 pb-1">
        {#if app.module === "mail"}<MailView />
        {:else}{#await views[app.module as Exclude<ModuleName, "mail">]() then m}<m.default />{/await}{/if}
      </main>
    </div>

    <footer class="bg-chrome text-muted-foreground flex h-6 shrink-0 items-center gap-4 px-3 text-[11px]">
      <span>{t(`module.${app.module}`)}</span>
      {#if app.module === "mail"}<span>{t("status.items", mail.total || mail.messages.length)}</span><span>{t("status.unread", app.unreadTotal)}</span>{/if}
      <span class="flex-1"></span>
      {#if app.bgSync && !app.status}<span class="flex items-center gap-1 opacity-80"><RefreshCw class="size-3 animate-spin" />{t("status.bg_sync")}</span>{/if}
      <span>{app.status || (app.accounts.length ? t("status.ready") : t("status.offline"))}</span>
      {#if app.accounts.length}
        <!-- Outlook durum çubuğu: Bağlandı / Bağlantı kesildi / Çevrimdışı çalışılıyor -->
        <span class="flex items-center gap-1 {app.net === 'online' ? '' : 'font-semibold text-amber-700 dark:text-amber-400'}" title={app.net === "disconnected" ? t("net.went_offline") : ""}>
          {#if app.net === "online"}<Wifi class="size-3" />{:else}<WifiOff class="size-3" />{/if}{t(`net.status_${app.net}`)}
        </span>
      {/if}
    </footer>
  </div>

  <Dialog.Root open={sourceText != null} onOpenChange={(o) => !o && (sourceText = null)}>
    <Dialog.Content class="flex h-[85vh] max-w-5xl flex-col sm:max-w-5xl">
      <Dialog.Header><Dialog.Title>{t("mail.view_source")}</Dialog.Title></Dialog.Header>
      <pre class="selectable bg-muted min-h-0 flex-1 overflow-auto rounded p-3 font-mono text-xs whitespace-pre-wrap">{sourceText}</pre>
    </Dialog.Content>
  </Dialog.Root>

  <Dialog.Root bind:open={shortcutsOpen}>
    <Dialog.Content class="sm:max-w-lg">
      <Dialog.Header><Dialog.Title>{t("help.shortcuts")}</Dialog.Title></Dialog.Header>
      <div class="grid grid-cols-[150px_1fr] gap-x-4 gap-y-1.5 text-sm">
        {#each [["Ctrl+N", "help.new_item"], ["Ctrl+Shift+M", "item.mail"], ["Ctrl+Shift+A", "item.event"], ["Ctrl+Shift+C", "item.contact"], ["Ctrl+Shift+K", "item.task"], ["Ctrl+R", "mail.reply"], ["Ctrl+Shift+R", "mail.reply_all"], ["Ctrl+F", "mail.forward"], ["Ctrl+Enter", "compose.send"], ["Ctrl+Q / Ctrl+U", "help.read_unread"], ["Delete / Shift+Delete", "help.delete"], ["Backspace", "mail.archive"], ["Insert", "mail.flag"], ["Ctrl+E", "help.search"], ["Ctrl+O", "menu.open"], ["F9", "mail.send_receive"], ["Ctrl+1 … Ctrl+5", "help.modules"], ["F1", "help.shortcuts"]] as [k, l] (k)}
          <kbd class="bg-muted rounded px-2 py-0.5 font-mono text-xs">{k}</kbd><span>{t(l)}</span>
        {/each}
      </div>
    </Dialog.Content>
  </Dialog.Root>

  {#await lazyAccountDialog() then m}<m.default />{/await}
  <DialogHost />
  <ContactCard />
{/if}
{#if IS_MAIN}<Toaster closeButton position="bottom-right" />{/if}
