// Uygulama genelindeki durum ve eylemler.
import { api, errorText } from "$lib/api";
import { t } from "$lib/i18n.svelte";
import type { Account, Category, ComposeData, Folder, FolderGroup, ModuleName, OutAttachment } from "$lib/types";
import { toast } from "svelte-sonner";
import { IS_MAIN, WINDOW_LABEL } from "$lib/window";
import { emit, emitTo, listen } from "@tauri-apps/api/event";

export interface ComposeRequest {
  key: number;
  mode: "new" | "reply" | "reply_all" | "forward" | "draft" | "edit";
  sourceId?: number;
  data?: Partial<ComposeData>;
  attachments?: OutAttachment[];
  /** Çökme kurtarmadan geri yüklenen iletinin kaydı */
  recoveryId?: string;
}

interface ConfirmReq {
  title: string;
  text?: string;
  ok?: string;
  cancel?: string;
  alt?: string;
  destructive?: boolean;
  /** Olası bir hatayı soran uyarı: varsayılan (Enter) güvenli seçenek "Vazgeç" */
  safe?: boolean;
  resolve: (v: boolean | "alt") => void;
}

interface PromptReq {
  title: string;
  label?: string;
  value: string;
  /** Parola: yazılanlar görünmez */
  secret?: boolean;
  resolve: (v: string | null) => void;
}

class AppState {
  module = $state<ModuleName>("mail");
  accounts = $state<Account[]>([]);
  categories = $state<Category[]>([]);
  settings = $state<Record<string, string>>({});
  folderGroups = $state<FolderGroup[]>([]);
  favorites = $state<Folder[]>([]);
  outbox = $state<Folder | null>(null);
  multiAccount = $state(false);
  unreadTotal = $state(0);
  status = $state("");
  /** Arka planda eşitleme sürüyor (durum çubuğunda küçük gösterge) */
  bgSync = $state(false);
  /** Bağlantı durumu (Outlook durum çubuğu): "online" | "disconnected" | "work_offline" */
  net = $state<"online" | "disconnected" | "work_offline">("online");
  private lastBadge = -1;
  search = $state("");
  /** Değiştiğinde modüller verilerini yeniden yükler */
  revision = $state({ mail: 0, calendar: 0, contacts: 0, tasks: 0, notes: 0 });


  /** Ana pencerede "Dosya" görünümü açık mı */
  backstage = $state(false);
  accountDialog = $state<{ open: boolean; account: Account | null; auto?: boolean }>({ open: false, account: null });
  settingsDialog = $state<{ open: boolean; tab: string }>({ open: false, tab: "general" });
  rulesPrefill = $state<{ from: string } | null>(null);
  /** "Kuralları Şimdi Çalıştır" penceresinin klasörü */
  runRulesFolder = $state<number | null>(null);
  confirmReq = $state<ConfirmReq | null>(null);
  promptReq = $state<PromptReq | null>(null);
  /** Diğer modüllerden "yeni öğe" isteği (ör. e-postadan toplantı) */
  newItemRequest = $state<{ module: ModuleName; prefill?: Record<string, unknown>; key: number } | null>(null);
  openItemRequest = $state<{ module: ModuleName; id: number; key: number } | null>(null);
  /** Takvimi bu güne götür (toplantı isteğindeki "Takvim...") */
  calendarGoto = $state<number | null>(null);

  private composeKey = 1;

  setting(key: string, def = ""): string {
    return this.settings[key] ?? def;
  }

  async setSetting(key: string, value: string) {
    this.settings[key] = value;
    await api.setSetting(key, value);
    this.announce();
  }

  // Her pencerenin kendi durum kopyası var: ayar/hesap/kategori değişince (ör. ayrı Ayarlar
  // penceresinde) diğer pencereler haberdar edilir ve yeniden yükler
  private announceTimer: ReturnType<typeof setTimeout> | undefined;
  private announce() {
    clearTimeout(this.announceTimer);
    this.announceTimer = setTimeout(() => void this.flushAnnounce(), 80);
  }

  /** Bekleyen bildirimi hemen gönderir (pencere kapanmadan önce) */
  async flushAnnounce() {
    clearTimeout(this.announceTimer);
    await emit("app://state-changed", { from: WINDOW_LABEL }).catch(() => {});
  }

  /** Başka pencerede yapılan değişiklikleri dinler; `after` yeniden yüklemeden sonra çalışır */
  async listenState(after: () => void | Promise<void>) {
    return listen<{ from: string }>("app://state-changed", async (e) => {
      if (e.payload.from === WINDOW_LABEL) return;
      await this.loadAll();
      await after();
    });
  }

  async loadAll() {
    const [accounts, categories, settings] = await Promise.all([api.listAccounts(), api.listCategories(), api.getSettings()]);
    this.accounts = accounts;
    this.categories = categories;
    this.settings = settings;
    await this.loadFolders();
  }

  async loadAccounts() {
    this.accounts = await api.listAccounts();
    await this.loadFolders();
    this.announce();
  }

  async loadCategories() {
    this.categories = await api.listCategories();
    this.announce();
  }

  async loadFolders() {
    const r = await api.folderTree();
    this.folderGroups = r.groups;
    this.favorites = r.favorites;
    this.outbox = r.outbox;
    this.multiAccount = r.multi;
    this.unreadTotal = r.unread_total;
    // Görev çubuğu sayacı: Gelen Kutusu'ndaki okunmamış iletiler (Outlook gibi)
    if (IS_MAIN) {
      const inbox = r.groups.flatMap((g) => g.folders).filter((f) => f.role === "inbox").reduce((n, f) => n + (f.unread || 0), 0);
      if (inbox !== this.lastBadge) {
        this.lastBadge = inbox;
        void api.setUnreadBadge(inbox).catch(() => {});
      }
    }
  }

  categoryColor(name: string): string {
    return this.categories.find((c) => c.name === name)?.color ?? "#888888";
  }

  defaultAccountId(): number | null {
    const d = Number(this.setting("default_account", "0"));
    if (d && this.accounts.some((a) => a.id === d)) return d;
    return this.accounts[0]?.id ?? null;
  }

  bump(module: keyof AppState["revision"]) {
    this.revision[module]++;
  }

  // ------------------------------------------------------------- pencereler
  /** Yeni ileti / yanıt / iletme: Outlook gibi ayrı pencerede açılır. */
  compose(req: Omit<ComposeRequest, "key"> = { mode: "new" }) {
    const title = req.data?.subject || t(req.mode === "new" ? "compose.untitled" : `mail.${req.mode === "draft" || req.mode === "edit" ? "draft" : req.mode}`);
    void api.openWindow("compose", { ...req, key: Date.now() + this.composeKey++ }, title).catch((e) => this.error(e));
  }

  /** İletiyi kendi penceresinde açar (çift tıklama, .msg dosyası, ek). */
  openMessage(id: number, subject = "", message?: unknown) {
    // Çok büyük gövdeler (gömülü resimler) aktarılmaz; pencere kendisi okur
    const pre = message && JSON.stringify(message).length < 4_000_000 ? message : undefined;
    void api.openWindow("read", pre ? { id, message: pre } : { id }, subject || "Zarf").catch((e) => this.error(e));
  }

  /** Ayarlar (Outlook Seçenekleri gibi) ayrı pencerede açılır; açıksa öne gelir */
  openSettings(tab: string, from?: string) {
    void api.openWindow("settings", { tab, from }, t("settings.options_title")).catch((e) => this.error(e));
  }

  /** "Kural oluştur": gönderen önceden doldurulmuş Kural Sihirbazı */
  openRulesFor(from: string) {
    this.openSettings("rules", from);
  }

  openAccountDialog() {
    if (IS_MAIN) {
      this.accountDialog = { open: true, account: null };
      return;
    }
    void emitTo("main", "app://open-account", {});
    void api.focusMain();
  }

  // ------------------------------------------------------------- iletişim kutuları
  confirm(title: string, text?: string, opts: { ok?: string; cancel?: string; destructive?: boolean; safe?: boolean } = {}): Promise<boolean> {
    return new Promise((resolve) => {
      this.confirmReq = { title, text, ...opts, resolve: (v) => resolve(v === true) };
    });
  }

  /** Üç seçenekli soru: "ok" | "alt" | "cancel" */
  choose(title: string, text: string, ok: string, alt: string): Promise<"ok" | "alt" | "cancel"> {
    return new Promise((resolve) => {
      this.confirmReq = { title, text, ok, alt, resolve: (v) => resolve(v === true ? "ok" : v === "alt" ? "alt" : "cancel") };
    });
  }

  prompt(title: string, label = "", value = "", secret = false): Promise<string | null> {
    return new Promise((resolve) => {
      this.promptReq = { title, label, value, secret, resolve };
    });
  }

  error(e: unknown) {
    toast.error(errorText(e));
  }

  info(msg: string) {
    toast.success(msg);
  }

  newItem(module: ModuleName, prefill?: Record<string, unknown>) {
    if (module === "mail") {
      this.compose({ mode: "new", data: prefill as Partial<ComposeData> });
      return;
    }
    this.module = module;
    this.newItemRequest = { module, prefill, key: Date.now() };
  }

  openItem(module: ModuleName, id: number) {
    this.module = module;
    this.openItemRequest = { module, id, key: Date.now() };
  }

  /** Dosya açma (komut satırı, sürükle-bırak, Dosya > Aç) */
  async openPath(path: string) {
    if (path.toLowerCase().startsWith("mailto:")) {
      this.openMailto(path);
      return;
    }
    try {
      const r = await api.openFile(path);
      switch (r.kind) {
        case "message":
          this.openMessage(r.id!);
          break;
        case "events":
          toast.success(t("import.events_done", r.count ?? 0));
          this.bump("calendar");
          this.module = "calendar";
          break;
        case "contacts":
          toast.success(t("import.contacts_done", r.count ?? 0));
          this.bump("contacts");
          this.module = "contacts";
          break;
        case "contact":
          this.bump("contacts");
          this.openItem("contacts", r.id!);
          break;
        case "task":
          this.bump("tasks");
          this.openItem("tasks", r.id!);
          break;
        case "imported":
          toast.success(t("import.messages_done", r.count ?? 0));
          this.bump("mail");
          break;
      }
    } catch (e) {
      this.error(e);
    }
  }

  openMailto(uri: string) {
    const u = uri.replace(/^mailto:/i, "");
    const [addr, query] = u.split("?");
    const params = new URLSearchParams(query ?? "");
    this.compose({
      mode: "new",
      data: {
        to: decodeURIComponent(addr ?? ""),
        cc: params.get("cc") ?? "",
        bcc: params.get("bcc") ?? "",
        subject: params.get("subject") ?? "",
        html: (params.get("body") ?? "").replace(/\n/g, "<br>"),
      },
    });
  }
}

export const app = new AppState();
