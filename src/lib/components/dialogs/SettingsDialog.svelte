<script lang="ts">
  import SignaturesEditor from "./SignaturesEditor.svelte";
  import TemplatesEditor from "./TemplatesEditor.svelte";
  import IdentitiesEditor from "./IdentitiesEditor.svelte";
  import { addTrustedSender, loadTrustedSenders, removeTrustedSender } from "$lib/trust";
  import { ALERT_ACTIONS, ALERT_BUTTON_DEFAULTS } from "$lib/alerts";
  // Ayarlar: genel, posta, takvim, hesaplar, imzalar, otomatik yanıtlar, kurallar, kategoriler, hakkında.
  import * as Dialog from "$lib/components/ui/dialog";
  import SecuritySettings from "./SecuritySettings.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Switch } from "$lib/components/ui/switch";
  import { Textarea } from "$lib/components/ui/textarea";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { i18n, t } from "$lib/i18n.svelte";
  import type { Account, Rule } from "$lib/types";
  import { fmtSize, initials } from "$lib/format";
  import { SCALES, autoScale } from "$lib/zoom";
  import { COMMON_FONTS, FONT_SIZES, fontCss, fontStack, parseFont, type FontKey, type FontSpec } from "$lib/fonts";
  import TypeIcon from "@lucide/svelte/icons/type";
  import { setMode } from "mode-watcher";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import XIcon from "@lucide/svelte/icons/x";
  import SettingsIcon from "@lucide/svelte/icons/settings-2";
  import MailIcon from "@lucide/svelte/icons/mail";
  import CalendarIcon from "@lucide/svelte/icons/calendar";
  import UsersIcon from "@lucide/svelte/icons/users";
  import LanguagesIcon from "@lucide/svelte/icons/languages";
  import SlidersIcon from "@lucide/svelte/icons/sliders-horizontal";
  import ShieldIcon from "@lucide/svelte/icons/shield-check";
  import { toast } from "svelte-sonner";
  import RuleWizard from "./RuleWizard.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  /** Ayrı pencerede (Outlook Seçenekleri gibi) gösteriliyor: Tamam/İptal pencereyi kapatır */
  let { windowed = false }: { windowed?: boolean } = $props();
  function closeWindowed() {
    if (windowed) void getCurrentWindow().destroy().catch(() => window.close());
  }

  // Outlook Seçenekleri'ndeki sırayla
  const NAV = ["general", "mail", "calendar", "people", "language", "advanced", "|", "accounts", "identities", "signatures", "templates", "autoreply", "rules", "categories", "|", "trust", "about"];
  /** Outlook düzeninde (sıkı satırlı) seçenek sayfaları */
  const OPTION_TABS = ["general", "mail", "calendar", "people", "language", "advanced", "trust", "stationery"];
  const ICONS: Record<string, typeof SettingsIcon> = { general: SettingsIcon, mail: MailIcon, calendar: CalendarIcon, people: UsersIcon, language: LanguagesIcon, advanced: SlidersIcon, trust: ShieldIcon, stationery: TypeIcon };

  // Outlook gibi: değişiklikler "Tamam" ile uygulanır, "İptal" vazgeçer
  let pending = $state<Record<string, string>>({});
  function get(key: string, def = ""): string {
    return pending[key] ?? app.setting(key, def);
  }
  function set(key: string, value: string) {
    pending = { ...pending, [key]: value };
  }
  async function commit() {
    const p = pending;
    pending = {};
    for (const [k, v] of Object.entries(p)) {
      if (k === "language") {
        await i18n.set(v);
        app.settings.language = v;
      } else if (k === "user_name") {
        // Kullanıcı adı = varsayılan hesabın görünen adı (gönderilen iletilerde "Kimden")
        const a = app.accounts.find((x) => x.id === app.defaultAccountId());
        if (a && a.display_name !== v) {
          await api.saveAccount({ ...a, display_name: v }, null).catch((e) => app.error(e));
          await app.loadAccounts();
        }
      } else if (k === "autostart") {
        await api.setAutostart(v === "1").catch(() => {});
        await app.setSetting(k, v);
      } else {
        await app.setSetting(k, v);
        if (k === "theme") setMode(v as "light" | "dark" | "system");
      }
    }
    app.settingsDialog = { ...app.settingsDialog, open: false };
    if (windowed) await app.flushAnnounce();
    closeWindowed();
  }
  function cancel() {
    pending = {};
    app.settingsDialog = { ...app.settingsDialog, open: false };
    closeWindowed();
  }

  let rules = $state<Rule[]>([]);
  let arAccount = $state<number | null>(null);
  let arEnabled = $state(false);
  let arText = $state("");
  let info = $state<{ version: string; data_dir: string; readpst: boolean } | null>(null);
  let newCatName = $state("");
  let newCatColor = $state("#3498db");

  const tab = $derived(app.settingsDialog.tab);

  async function addTrusted() {
    const v = await app.prompt(t("settings.trusted_add"), t("settings.trusted_label"));
    if (v?.trim() && !(await addTrustedSender(v))) toast.error(t("settings.trusted_invalid", v.trim()));
  }

  // Veritabanı boyutu ve sıkıştırma (Gelişmiş → Veritabanı)
  let storage = $state<{ size: number; free: number; incremental: boolean } | null>(null);
  let compacting = $state(false);
  $effect(() => {
    if (app.settingsDialog.open && tab === "advanced") void api.storageInfo().then((s) => (storage = s)).catch(() => {});
  });
  async function compactDb() {
    if (!(await app.confirm(t("settings.compact_confirm_title"), t("settings.compact_confirm_text"), { ok: t("settings.compact") }))) return;
    compacting = true;
    try {
      const r = await api.compactDatabase();
      toast.success(t("settings.compact_done", fmtSize(r.before), fmtSize(r.after)));
      storage = await api.storageInfo();
    } catch (e) {
      app.error(e);
    } finally {
      compacting = false;
    }
  }

  // Varsayılan e-posta uygulaması (Genel → Başlangıç)
  let isDefault = $state(true);
  $effect(() => {
    if (app.settingsDialog.open && tab === "general") void api.defaultAppStatus().then((v) => (isDefault = v)).catch(() => {});
  });
  async function makeDefault() {
    try {
      await api.setDefaultApp();
      isDefault = await api.defaultAppStatus();
      toast.success(t("default_app.done"));
    } catch (e) {
      app.error(e);
    }
  }

  $effect(() => {
    if (!app.settingsDialog.open) return;
    void api.listRules().then((r) => (rules = r));
    // Exchange kuralları açılışta sunucudan tazelenir
    if (app.accounts.some((a) => a.kind === "exchange")) void api.refreshServerRules().then(async () => (rules = await api.listRules())).catch(() => {});
    void api.appInfo().then((r) => (info = r));
    if (app.rulesPrefill) {
      const from = app.rulesPrefill.from;
      const addr = from.replace(/^.*<([^>]+)>.*$/, "$1");
      app.rulesPrefill = null;
      const ex = app.accounts.find((a) => a.kind === "exchange");
      if (ex) openWizard(0, ex.id, t("rules.from_name", addr), true, from);
      else openLocal(null, addr);
    }
    if (arAccount == null && app.accounts.length) pickAr(app.accounts[0].id);
  });

  // Exchange kuralları Kural Sihirbazı ile düzenlenir
  let wizard = $state<{ open: boolean; id: number; account: number; name: string; enabled: boolean; from: string; local: boolean; rule: Rule | null }>({ open: false, id: 0, account: 0, name: "", enabled: true, from: "", local: false, rule: null });
  function openWizard(id: number, account: number, name = "", enabled = true, from = "") {
    wizard = { open: true, id, account, name, enabled, from, local: false, rule: null };
  }
  /** Bu bilgisayarda çalışan kural (IMAP, tüm hesaplar) */
  function openLocal(r: Rule | null, from = "") {
    const name = r?.name ?? (from ? t("rules.from_name", from) : "");
    wizard = { open: true, id: r?.id ?? 0, account: 0, name, enabled: r?.enabled ?? true, from, local: true, rule: r };
  }
  const exchangeAccounts = $derived(app.accounts.filter((a) => a.kind === "exchange"));
  /** Kendi kimliğimizle açılan Exchange hesapları: paylaşılan kutular bunlardan birine bağlanır */
  const baseAccounts = $derived(exchangeAccounts.filter((a) => a.shared_of == null));

  // Paylaşılan posta kutusu ekleme
  let sharedOpen = $state(false);
  let sharedBase = $state<number | null>(null);
  let sharedEmail = $state("");
  let sharedName = $state("");
  function openShared() {
    sharedBase = baseAccounts[0]?.id ?? null;
    sharedEmail = "";
    sharedName = "";
    sharedOpen = true;
  }
  async function addShared() {
    if (sharedBase == null) return;
    try {
      await api.addSharedMailbox(sharedBase, sharedEmail, sharedName);
      await app.loadAccounts();
      sharedOpen = false;
      toast.success(t("shared.added", sharedEmail.trim()));
    } catch (e) {
      app.error(e);
    }
  }
  function newRule() {
    const ex = exchangeAccounts[0];
    if (ex) openWizard(0, ex.id);
    else openLocal(null);
  }
  function editRule(r: Rule) {
    if (r.account_id != null && (r.remote_id || isExchange(r.account_id))) openWizard(r.id, r.account_id, r.name, r.enabled);
    else openLocal(r);
  }

  function bool(key: string, def = false) {
    return get(key, def ? "1" : "0") === "1";
  }

  // Kırtasiye ve Yazı Tipleri: bu bilgisayardaki yazı tipleri de listelenir
  let systemFonts = $state<string[]>([]);
  $effect(() => {
    if (tab === "stationery" && !systemFonts.length) api.systemFonts().then((f) => (systemFonts = f.filter((x) => !COMMON_FONTS.includes(x)))).catch(() => {});
  });
  function fontOf(key: FontKey): FontSpec {
    return parseFont(get(key, ""), key);
  }
  function setFont(key: FontKey, patch: Partial<FontSpec>) {
    set(key, JSON.stringify({ ...fontOf(key), ...patch }));
  }


  function pickAr(id: number) {
    arAccount = id;
    const a = app.accounts.find((x) => x.id === id);
    arEnabled = a?.auto_reply_enabled ?? false;
    arText = a?.auto_reply_text ?? "";
  }

  async function saveAccountField(id: number | null, patch: Partial<Account>) {
    const a = app.accounts.find((x) => x.id === id);
    if (!a) return;
    await api.saveAccount({ ...a, ...patch }, null);
    await app.loadAccounts();
    toast.success(t("common.saved"));
  }

  async function saveAutoReply() {
    if (arAccount == null) return;
    try {
      // Exchange hesaplarında sunucudaki "Ofis Dışında" ayarı da güncellenir
      await api.setAutoReply(arAccount, arEnabled, arText);
      await app.loadAccounts();
      toast.success(t("common.saved"));
    } catch (e) {
      app.error(e);
    }
  }

  async function deleteAccount(a: Account) {
    if (await app.confirm(t("account.delete_confirm", a.email), t("account.delete_confirm_text"), { ok: t("common.delete"), destructive: true })) {
      await api.deleteAccount(a.id);
      await app.loadAccounts();
      app.bump("mail");
    }
  }

  const isExchange = (accountId: number | null | undefined) => app.accounts.some((a) => a.id === accountId && a.kind === "exchange");

  /** Sunucu işlemi: Outlook masaüstü istemci kuralları varsa önce kullanıcıya sorulur. */
  async function serverOp(fn: (force: boolean) => Promise<unknown>) {
    try {
      await fn(false);
    } catch (e) {
      if (String(e).includes("rules.blob_exists")) {
        if (await app.confirm(t("rules.blob_title"), t("rules.blob_text"), { ok: t("common.continue"), destructive: true })) await fn(true);
      } else {
        throw e;
      }
    }
  }

  async function deleteRule(r: Rule) {
    try {
      await serverOp((force) => api.deleteRule(r.id, force));
    } catch (e) {
      app.error(e);
    }
    rules = await api.listRules();
  }

  async function toggleRule(r: Rule) {
    try {
      await serverOp((force) => api.saveRule({ ...r, enabled: !r.enabled }, force));
    } catch (e) {
      app.error(e);
    }
    rules = await api.listRules();
  }

  async function addCategory() {
    if (!newCatName.trim()) return;
    await api.saveCategory(null, newCatName.trim(), newCatColor);
    newCatName = "";
    await app.loadCategories();
  }

  async function renameCategory(name: string, color: string) {
    const n = await app.prompt(t("categories.rename"), t("categories.name"), name);
    if (n) {
      await api.saveCategory(name, n, color);
      await app.loadCategories();
    }
  }
</script>

<!-- Outlook Seçenekleri gibi: solda kategoriler, sağda başlıklı bölümler, altta Tamam / İptal -->
{#snippet section(title: string)}
  <div class="mt-5 mb-2 first:mt-0">
    <div class="text-[13px] font-semibold">{title}</div>
    <div class="bg-border mt-1 h-px"></div>
  </div>
{/snippet}
{#snippet check(key: string, label: string, def = false, after?: (v: boolean) => void)}
  <label class="flex items-center gap-2 py-[3px] pl-4 text-[13px]">
    <input type="checkbox" class="accent-primary size-[15px]" checked={bool(key, def)} onchange={(e) => { const v = (e.target as HTMLInputElement).checked; set(key, v ? "1" : "0"); after?.(v); }} />
    {label}
  </label>
{/snippet}
{#snippet pick(key: string, label: string, def: string, options: [string, string][], wide = false)}
  <div class="flex items-center gap-2 py-[3px] pl-4 text-[13px]">
    <span class="{wide ? 'w-64' : 'w-36'} shrink-0">{label}:</span>
    <select class="h-[26px] min-w-56 rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] dark:border-white/30 dark:bg-transparent" value={get(key, def)} onchange={(e) => set(key, (e.target as HTMLSelectElement).value)}>
      {#each options as [v, l] (v)}<option value={v}>{l}</option>{/each}
    </select>
  </div>
{/snippet}
{#snippet fontRow(key: FontKey, label: string)}
  {@const f = fontOf(key)}
  <div class="py-2 pl-4 text-[13px]">
    <div class="mb-1.5">{label}</div>
    <div class="flex flex-wrap items-center gap-2">
      <select class="h-[26px] w-52 rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] dark:border-white/30 dark:bg-transparent" value={f.family} onchange={(e) => setFont(key, { family: (e.target as HTMLSelectElement).value })} title={t("editor.font")}>
        {#if !COMMON_FONTS.includes(f.family) && !systemFonts.includes(f.family)}<option value={f.family}>{f.family}</option>{/if}
        {#each COMMON_FONTS as n (n)}<option value={n} style="font-family:{fontStack(n)}">{n}</option>{/each}
        {#if systemFonts.length}<optgroup label={t("compose.fonts_system")}>{#each systemFonts as n (n)}<option value={n} style="font-family:{fontStack(n)}">{n}</option>{/each}</optgroup>{/if}
      </select>
      <select class="h-[26px] w-20 rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] dark:border-white/30 dark:bg-transparent" value={String(f.size)} onchange={(e) => setFont(key, { size: Number((e.target as HTMLSelectElement).value) })} title={t("editor.size")}>
        {#each FONT_SIZES as z (z)}<option value={String(z)}>{String(z).replace(".", ",")}</option>{/each}
      </select>
      <button class="h-[26px] w-8 rounded-[3px] border font-bold {f.bold ? 'bg-primary/15 border-primary' : 'border-[#8a8a8a] hover:bg-accent dark:border-white/30'}" title={t("editor.bold")} onclick={() => setFont(key, { bold: !f.bold })}>K</button>
      <button class="h-[26px] w-8 rounded-[3px] border italic {f.italic ? 'bg-primary/15 border-primary' : 'border-[#8a8a8a] hover:bg-accent dark:border-white/30'}" title={t("editor.italic")} onclick={() => setFont(key, { italic: !f.italic })}>T</button>
      <label class="relative flex h-[26px] items-center gap-1.5 rounded-[3px] border border-[#8a8a8a] px-2 hover:bg-accent dark:border-white/30" title={t("settings.font_color")}>
        <span class="size-3.5 rounded-sm border" style="background:{f.color || '#000000'}"></span>{f.color ? t("settings.font_color") : t("settings.font_color_auto")}
        <input type="color" class="absolute inset-0 cursor-pointer opacity-0" value={f.color || "#000000"} onchange={(e) => setFont(key, { color: (e.target as HTMLInputElement).value })} />
      </label>
      {#if f.color}<button class="text-primary text-[12px] hover:underline" onclick={() => setFont(key, { color: "" })}>{t("settings.font_color_auto")}</button>{/if}
    </div>
    <div class="mt-2 max-w-md truncate rounded-[3px] border bg-white px-3 py-2 text-black" style={fontCss(f)}>{t("settings.font_sample")}</div>
  </div>
{/snippet}
{#snippet intro(text: string)}
  {@const Icon = ICONS[tab] ?? SettingsIcon}
  <div class="mb-4 flex items-center gap-3 text-[14px]"><Icon class="text-primary size-8" strokeWidth={1.2} />{text}</div>
{/snippet}
{#snippet field(label: string, value: string, oninput: (v: string) => void, cls = "w-48")}
  <div class="flex items-center gap-2 py-1 pl-4 text-[13px]">
    <span class="w-36 shrink-0">{label}:</span>
    <input class="h-[26px] rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] outline-none focus:border-primary dark:border-white/30 dark:bg-transparent {cls}" {value} oninput={(e) => oninput((e.target as HTMLInputElement).value)} />
  </div>
{/snippet}

<!-- Ayrı pencerede Esc İptal gibi -->
<svelte:window onkeydown={(e) => {
  if (!windowed || e.defaultPrevented || wizard.open || app.accountDialog.open || app.confirmReq || app.promptReq) return;
  if (e.key === "Escape") cancel();
  // Outlook Seçenekleri: tek satırlı alandayken Enter = Tamam
  else if (e.key === "Enter" && !e.isComposing && (e.target as HTMLElement)?.tagName === "INPUT" && !["checkbox", "radio"].includes((e.target as HTMLInputElement).type)) { e.preventDefault(); void commit(); }
}} />
{#if windowed}
  <!-- Ayrı pencere: başlık çubuğu sürüklenebilir -->
  <div class="bg-background flex h-screen flex-col">
    {@render body()}
  </div>
{:else}
<Dialog.Root open={app.settingsDialog.open} onOpenChange={(o) => { if (!o) cancel(); }}>
  <Dialog.Content class="flex h-[min(680px,90vh)] w-[min(900px,96vw)] max-w-none flex-col gap-0 rounded-md p-0 sm:max-w-none" showCloseButton={false}>
    {@render body()}
  </Dialog.Content>
</Dialog.Root>
{/if}

{#snippet body()}
    <div class="flex h-9 shrink-0 items-center border-b pl-3 text-[13px]" data-tauri-drag-region>
      {#if windowed}<span class="flex-1 text-[13px]" data-tauri-drag-region>{t("settings.options_title")}</span>{:else}<Dialog.Title class="flex-1 text-[13px] font-normal">{t("settings.options_title")}</Dialog.Title>{/if}
      <button class="hover:bg-accent flex h-full w-10 items-center justify-center" title={t("help.shortcuts")} onclick={() => window.dispatchEvent(new Event("lynora:shortcuts"))}>?</button>
      <button class="flex h-full w-11 items-center justify-center hover:bg-[#c42b1c] hover:text-white" title={t("common.close")} onclick={cancel}><XIcon class="size-4" /></button>
    </div>
    <div class="flex min-h-0 flex-1">
      <nav class="bg-chrome w-44 shrink-0 overflow-y-auto border-r py-2 text-[13px]">
        {#each NAV as tb, i (tb + i)}
          {#if tb === "|"}
            <div class="bg-border mx-3 my-1.5 h-px"></div>
          {:else}
            <button class="block w-full px-3 py-1.5 text-left {tab === tb ? 'bg-background mx-1 w-[calc(100%-0.5rem)] rounded-[3px] border border-[#1b1b1b] dark:border-white/70' : 'hover:bg-black/5 dark:hover:bg-white/10'}" onclick={() => (app.settingsDialog = { open: true, tab: tb })}>{t(`settings.tab_${tb}`)}</button>
          {/if}
        {/each}
      </nav>
      <div class="min-h-0 flex-1 overflow-y-auto px-6 py-5 text-sm {OPTION_TABS.includes(tab) ? '' : 'space-y-3'}">
      {#if tab === "general"}
        {@const me = app.accounts.find((x) => x.id === app.defaultAccountId())}
        {@render intro(t("settings.general_intro"))}
        {@render section(t("settings.sec_ui"))}
        {@render check("ribbon_collapsed", t("settings.ribbon_collapsed"))}
        {@render check("show_avatars", t("view.show_avatars"), true)}
        {@render pick("reading_pane", t("settings.reading_pane"), "right", [["right", t("settings.reading_pane_right")], ["bottom", t("settings.reading_pane_bottom")], ["off", t("settings.reading_pane_off")]])}
        {@render pick("ui_scale", t("settings.ui_scale"), "auto", [["auto", t("settings.ui_scale_auto", Math.round(autoScale() * 100))], ...SCALES.map((v) => [v, `%${Math.round(Number(v) * 100)}`] as [string, string])])}
        {@render section(t("settings.sec_personalize"))}
        {@render field(t("settings.user_name"), get("user_name", me?.display_name ?? ""), (v) => set("user_name", v), "w-64")}
        {@render field(t("settings.initials"), get("user_initials", initials(me?.display_name || me?.email || "")), (v) => set("user_initials", v), "w-20")}
        {@render pick("theme", t("settings.office_theme"), "system", [["system", t("settings.theme_system")], ["light", t("settings.theme_light")], ["dark", t("settings.theme_dark")]])}
        {@render section(t("settings.sec_privacy"))}
        <div class="py-1 pl-4">
          <button class="h-[26px] rounded-[3px] border border-[#8a8a8a] px-3 text-[13px] hover:bg-[#e5f1fb] dark:border-white/30 dark:hover:bg-white/10" onclick={() => (app.settingsDialog = { open: true, tab: "trust" })}>{t("settings.privacy_btn")}</button>
        </div>
        {@render section(t("settings.sec_startup"))}
        {@render pick("start_module", t("settings.when_opens"), "mail", ["mail", "calendar", "contacts", "tasks", "notes"].map((m) => [m, t(`module.${m}`)]))}
        {@render check("autostart", t("settings.autostart"))}
        {@render check("sync_on_start", t("settings.sync_on_start"), true)}
        <div class="flex items-center gap-3 py-1.5 pl-4 text-[13px]">
          <span>{isDefault ? t("default_app.is_default") : t("default_app.not_default")}</span>
          {#if !isDefault}
            <button class="h-[26px] rounded-[3px] border border-[#8a8a8a] px-3 text-[13px] hover:bg-[#e5f1fb] dark:border-white/30 dark:hover:bg-white/10" onclick={makeDefault}>{t("default_app.button")}</button>
          {/if}
        </div>
      {:else if tab === "language"}
        {@render intro(t("settings.language_intro"))}
        {@render section(t("settings.sec_display_language"))}
        <div class="flex items-center gap-2 py-1 pl-4 text-[13px]">
          <span class="w-36 shrink-0">{t("settings.language")}:</span>
          <select class="h-[26px] min-w-56 rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] dark:border-white/30 dark:bg-transparent" value={get("language", "auto")} onchange={(e) => set("language", (e.target as HTMLSelectElement).value)}>
            <option value="auto">{t("settings.language_auto")}</option>
            {#each i18n.available as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
          </select>
        </div>
        <p class="text-muted-foreground pl-4 text-xs">{t("settings.language_help")}</p>
      {:else if tab === "people"}
        {@render intro(t("settings.people_intro"))}
        {@render section(t("settings.sec_photos"))}
        {@render check("show_avatars", t("settings.show_photos"), true)}
        {@render section(t("settings.sec_names"))}
        <p class="text-muted-foreground py-1 pl-4 text-[13px]">{t("settings.names_help")}</p>
      {:else if tab === "advanced"}
        {@render intro(t("settings.advanced_intro"))}
        {@render section(t("settings.sec_sync"))}
        {@render pick("sync_interval", t("settings.sync_interval"), "10", ["0", "1", "5", "10", "15", "30", "60"].map((v) => [v, v === "0" ? t("settings.manual") : t("settings.every_n_min", v)]))}
        {@render check("sync_on_select", t("settings.sync_on_select"), true)}
        {@render section(t("settings.sec_conversations"))}
        {@render check("conversations", t("view.conversations"))}
        {@render check("group_by_date", t("view.group_by_date"), true)}
        {@render section(t("settings.sec_display"))}
        {@render check("hw_accel", t("settings.hw_accel"), true)}
        <p class="text-muted-foreground py-1 pl-10 text-[12px]">{t("settings.hw_accel_help")}</p>
        {@render section(t("settings.sec_storage"))}
        <div class="py-1 pl-4 text-[13px]" data-testid="storage">
          {#if storage}<div>{t("settings.storage_size", fmtSize(storage.size), fmtSize(storage.free))}</div>{/if}
          <button class="mt-1.5 h-[26px] rounded-[3px] border border-[#8a8a8a] px-3 text-[13px] hover:bg-[#e5f1fb] disabled:opacity-50 dark:border-white/30 dark:hover:bg-white/10" disabled={compacting} onclick={compactDb}>{compacting ? t("settings.compacting") : t("settings.compact")}</button>
          <p class="text-muted-foreground py-1 text-[12px]">{storage?.incremental ? t("settings.compact_help_auto") : t("settings.compact_help")}</p>
        </div>
      {:else if tab === "trust"}
        {@render intro(t("settings.trust_intro"))}
        {@render section(t("settings.sec_download"))}
        {@render check("load_remote", t("settings.load_remote"))}
        {@render section(t("settings.sec_email_security"))}
        {@render check("plain_text_only", t("settings.plain_text_only"))}
        {@render section(t("settings.sec_encryption"))}
        <SecuritySettings {get} {set} />
        {@render section(t("settings.sec_trusted_senders"))}
        <div class="max-w-md space-y-1.5 py-1 pl-4 text-[13px]">
          <p class="text-muted-foreground text-xs">{t("settings.trusted_help")}</p>
          <div class="min-h-16 rounded-[3px] border border-[#8a8a8a] bg-white dark:border-white/30 dark:bg-transparent">
            {#each loadTrustedSenders() as s (s)}
              <div class="flex items-center gap-2 px-2 py-0.5">
                <span class="min-w-0 flex-1 truncate">{s}</span>
                <button class="hover:bg-accent rounded p-0.5" title={t("common.delete")} aria-label={t("common.delete")} onclick={() => removeTrustedSender(s)}><Trash2 class="size-3.5" /></button>
              </div>
            {:else}
              <p class="text-muted-foreground p-2 text-xs">{t("settings.trusted_none")}</p>
            {/each}
          </div>
          <button class="hover:bg-accent h-7 rounded border px-3" onclick={addTrusted}>{t("settings.trusted_add")}</button>
        </div>
      {:else if tab === "mail"}
        {@render intro(t("settings.mail_intro"))}
        {@render section(t("settings.sec_compose"))}
        {@render pick("compose_format", t("settings.compose_format"), "html", [["html", "HTML"], ["plain", t("compose.plain_text")]], true)}
        <div class="flex items-center gap-3 py-1 pl-4 text-[13px]">
          <span class="w-64 shrink-0">{t("settings.signatures_btn")}</span>
          <button class="hover:bg-accent h-7 rounded border px-3" onclick={() => (app.settingsDialog = { open: true, tab: "signatures" })}>{t("settings.tab_signatures")}…</button>
        </div>
        <div class="flex items-center gap-3 py-1 pl-4 text-[13px]">
          <span class="w-64 shrink-0">{t("settings.stationery_btn")}</span>
          <button class="hover:bg-accent h-7 rounded border px-3" onclick={() => (app.settingsDialog = { open: true, tab: "stationery" })}>{t("settings.tab_stationery")}…</button>
        </div>
        {@render section(t("settings.sec_reading"))}
        {@render pick("list_font", t("settings.list_font"), "normal", [["small", t("settings.size_small")], ["normal", t("settings.size_normal")], ["large", t("settings.size_large")]], true)}
        {@render pick("preview_lines", t("settings.preview_lines"), "1", [["0", t("settings.preview_off")], ["1", t("settings.preview_n", 1)], ["2", t("settings.preview_n", 2)], ["3", t("settings.preview_n", 3)]], true)}
        {@render pick("reading_zoom", t("settings.reading_zoom"), "100", ["75", "90", "100", "110", "125", "150", "175", "200"].map((v) => [v, `%${v}`]), true)}
        {@render pick("mark_read_delay", t("settings.mark_read"), "0", [["0", t("settings.mark_read_immediately")], ["2", t("settings.mark_read_after", 2)], ["5", t("settings.mark_read_after", 5)], ["10", t("settings.mark_read_after", 10)], ["-1", t("settings.mark_read_never")]], true)}
        {@render section(t("settings.sec_arrival"))}
        {@render check("notify_new_mail", t("settings.notifications"), true)}
        {@render pick("notify_style", t("settings.notify_style"), "alerts", [["alerts", t("settings.notify_alerts")], ["system", t("settings.notify_system")]], true)}
        {@render pick("alert_seconds", t("settings.alert_seconds"), "7", ["5", "7", "10", "15", "30"].map((v) => [v, t("settings.undo_n_sec", v)]), true)}
        {#each ALERT_BUTTON_DEFAULTS as d, i (i)}
          {@render pick(`alert_btn${i + 1}`, t("settings.alert_button", i + 1), d, [...Object.entries(ALERT_ACTIONS).map(([k, l]): [string, string] => [k, t(l)]), ["none", t("settings.alert_button_none")] as [string, string]], true)}
        {/each}
        {@render check("notify_sound", t("settings.notify_sound"), true)}
        {@render section(t("settings.sec_replies"))}
        {@render check("reply_inline", t("settings.reply_inline"), true)}
        {@render check("open_next_after_delete", t("settings.open_next_after_delete"), false)}
        {@render section(t("settings.sec_send"))}
        {@render pick("default_account", t("settings.default_account"), String(app.defaultAccountId() ?? ""), app.accounts.map((a) => [String(a.id), a.email]), true)}
        {@render pick("undo_send", t("settings.undo_send"), "0", ["0", "5", "10", "20", "30"].map((v) => [v, v === "0" ? t("settings.undo_off") : t("settings.undo_n_sec", v)]), true)}
      {:else if tab === "stationery"}
        {@render intro(t("settings.stationery_intro"))}
        {@render section(t("settings.sec_font_new"))}
        {@render fontRow("font_new", t("settings.font_new"))}
        {@render section(t("settings.sec_font_reply"))}
        {@render fontRow("font_reply", t("settings.font_reply"))}
        {@render section(t("settings.sec_font_plain"))}
        {@render fontRow("font_plain", t("settings.font_plain"))}
        <div class="py-2 pl-4"><button class="hover:bg-accent h-7 rounded border px-3 text-[13px]" onclick={() => (app.settingsDialog = { open: true, tab: "mail" })}>← {t("settings.tab_mail")}</button></div>
      {:else if tab === "calendar"}
        {@render intro(t("settings.calendar_intro"))}
        {@render section(t("settings.sec_work_time"))}
        <div class="flex items-center gap-3 py-1 pl-4 text-[13px]">
          <span class="w-64 shrink-0">{t("settings.work_hours")}</span>
          <input type="number" min="0" max="23" class="h-7 w-16 rounded border bg-transparent px-1.5" value={get("work_start", "8")} onchange={(e) => set("work_start", (e.target as HTMLInputElement).value)} />
          –
          <input type="number" min="1" max="24" class="h-7 w-16 rounded border bg-transparent px-1.5" value={get("work_end", "17")} onchange={(e) => set("work_end", (e.target as HTMLInputElement).value)} />
        </div>
        {@render section(t("settings.sec_cal_options"))}
        {@render pick("default_reminder", t("settings.default_reminder"), "15", ["-1", "0", "5", "10", "15", "30", "60", "1440"].map((v) => [v, v === "-1" ? t("reminder.none") : t("reminder.minutes", v)]), true)}
        {@render pick("default_duration", t("settings.default_duration"), "30", ["15", "30", "45", "60", "90", "120"].map((v) => [v, t("reminder.minutes", v)]), true)}
        {@render check("reminder_alerts", t("settings.reminder_alerts"), true)}
        {@render check("reminder_sound", t("settings.reminder_sound"), true)}
        {@render section(t("settings.sec_display"))}
        <div class="flex items-center gap-3 py-1 pl-4 text-[13px]">
          <span class="w-64 shrink-0">{t("settings.hour_height")}</span>
          <input type="range" min="32" max="96" step="4" value={get("hour_height", "48")} onchange={(e) => set("hour_height", (e.target as HTMLInputElement).value)} />
        </div>
      {:else if tab === "accounts"}
        <div class="flex items-center justify-between pr-8">
          <h3 class="text-lg font-semibold">{t("settings.tab_accounts")}</h3>
          <Button size="sm" onclick={() => (app.accountDialog = { open: true, account: null })}><Plus class="size-4" />{t("account.add")}</Button>
        </div>
        <div class="divide-y rounded-md border">
          {#each app.accounts as a (a.id)}
            <div class="flex items-center gap-3 p-3">
              <div class="flex-1">
                <div class="font-medium">{a.name}</div>
                <div class="text-muted-foreground text-xs">{a.email} · {a.kind === "exchange" ? "Exchange" : a.imap_host} · {a.shared_of != null ? t("shared.badge") : t(`account.auth_${a.auth_method || "password"}`)}</div>
                {#if a.auth_state === "failed"}<div class="text-xs text-amber-700 dark:text-amber-300">{t("account.paused_short")}</div>{/if}
              </div>
              {#if a.auth_state === "failed"}
                <Button size="sm" variant="outline" onclick={async () => { await api.retryAccount(a.id); await app.loadAccounts(); }}>{t("account.retry")}</Button>
              {/if}
              <Button size="sm" variant="outline" onclick={() => api.syncAccount(a.id)}>{t("account.sync")}</Button>
              <Button size="icon-sm" variant="ghost" onclick={() => (app.accountDialog = { open: true, account: a })}><Pencil class="size-4" /></Button>
              <Button size="icon-sm" variant="ghost" onclick={() => deleteAccount(a)}><Trash2 class="size-4" /></Button>
            </div>
          {:else}
            <div class="text-muted-foreground p-6 text-center">{t("account.none")}</div>
          {/each}
        </div>
        {#if baseAccounts.length}
          {#if sharedOpen}
            <div class="grid gap-2 rounded-md border p-3">
              <div class="font-medium">{t("shared.add")}</div>
              <p class="text-muted-foreground text-xs">{t("shared.help")}</p>
              <label class="flex items-center gap-2">
                <span class="w-40 shrink-0">{t("shared.base")}</span>
                <select class="border-input h-9 min-w-0 flex-1 rounded-md border bg-transparent px-2" bind:value={sharedBase}>
                  {#each baseAccounts as a (a.id)}<option value={a.id}>{a.email}</option>{/each}
                </select>
              </label>
              <label class="flex items-center gap-2"><span class="w-40 shrink-0">{t("shared.email")}</span><Input class="min-w-0 flex-1" bind:value={sharedEmail} /></label>
              <label class="flex items-center gap-2"><span class="w-40 shrink-0">{t("shared.name")}</span><Input class="min-w-0 flex-1" bind:value={sharedName} /></label>
              <div class="flex gap-2"><Button size="sm" disabled={!sharedEmail.trim()} onclick={addShared}>{t("common.add")}</Button><Button size="sm" variant="outline" onclick={() => (sharedOpen = false)}>{t("common.cancel")}</Button></div>
            </div>
          {:else}
            <Button size="sm" variant="outline" onclick={openShared}><Plus class="size-4" />{t("shared.add")}</Button>
          {/if}
        {/if}
        <details class="rounded-md border p-3">
          <summary class="cursor-pointer font-medium">{t("settings.oauth_title")}</summary>
          <p class="text-muted-foreground mt-2 text-xs">{t("settings.oauth_help")}</p>
          <div class="mt-3 grid grid-cols-[220px_1fr] items-center gap-2">
            <Label>{t("settings.oauth_ms_client_id")}</Label>
            <Input value={app.setting("oauth_ms_client_id", "")} placeholder="00000000-0000-0000-0000-000000000000" onchange={(e) => app.setSetting("oauth_ms_client_id", (e.target as HTMLInputElement).value.trim())} />
            <Label>{t("settings.oauth_ms_tenant")}</Label>
            <Input value={app.setting("oauth_ms_tenant", "")} placeholder="organizations" onchange={(e) => app.setSetting("oauth_ms_tenant", (e.target as HTMLInputElement).value.trim())} />
            <Label>{t("settings.oauth_google_client_id")}</Label>
            <Input value={app.setting("oauth_google_client_id", "")} onchange={(e) => app.setSetting("oauth_google_client_id", (e.target as HTMLInputElement).value.trim())} />
          </div>
        </details>
      {:else if tab === "signatures"}
        <SignaturesEditor />
      {:else if tab === "templates"}
        <TemplatesEditor />
      {:else if tab === "identities"}
        <IdentitiesEditor />
      {:else if tab === "autoreply"}
        <h3 class="text-lg font-semibold">{t("settings.tab_autoreply")}</h3>
        <p class="text-muted-foreground">{t("autoreply.help")}</p>
        {#if app.accounts.length}
          <select class="border-input h-9 rounded-md border bg-transparent px-2" value={arAccount} onchange={(e) => pickAr(Number((e.target as HTMLSelectElement).value))}>
            {#each app.accounts as a (a.id)}<option value={a.id}>{a.email}</option>{/each}
          </select>
          <label class="flex items-center gap-2"><Switch bind:checked={arEnabled} />{t("autoreply.enable")}</label>
          <Textarea rows={8} bind:value={arText} placeholder={t("autoreply.placeholder")} />
          <Button onclick={saveAutoReply}>{t("common.save")}</Button>
        {:else}<p class="text-muted-foreground">{t("account.none")}</p>{/if}
      {:else if tab === "rules"}
        <div class="flex items-center justify-between pr-8">
          <h3 class="text-lg font-semibold">{t("rules.title")}</h3>
          <div class="flex gap-2">
            {#if exchangeAccounts.length && app.accounts.some((a) => a.kind !== "exchange")}
              <Button size="sm" variant="outline" onclick={() => openLocal(null)}>{t("rules.new_local")}</Button>
            {/if}
            {#if exchangeAccounts.length > 1}
              {#each exchangeAccounts as a (a.id)}<Button size="sm" variant="outline" onclick={() => openWizard(0, a.id)}><Plus class="size-4" />{a.email}</Button>{/each}
            {:else}
              <Button size="sm" onclick={newRule}><Plus class="size-4" />{t("rules.new")}</Button>
            {/if}
          </div>
        </div>
        <p class="text-muted-foreground text-xs">{t("rules.help")}</p>
        <div class="divide-y rounded-md border">
          {#each rules as r (r.id)}
            <div class="flex items-center gap-3 p-2">
              <Switch checked={r.enabled} onCheckedChange={() => toggleRule(r)} />
              <div class="flex-1">
                <div class="flex items-center gap-2 font-medium">
                  {r.name}
                  {#if r.remote_id}<span class="rounded bg-blue-100 px-1.5 py-0.5 text-[10px] font-normal text-blue-800 dark:bg-blue-950 dark:text-blue-200">Exchange</span>{/if}
                </div>
                <div class="text-muted-foreground text-xs">
                  {#if r.spec}{t("rules.advanced")}{:else if r.complex}{t("rules.complex_editable")}{:else}{t(`rules.field_${r.field}`)} {t(`rules.op_${r.op}`)} “{r.value}” → {t(`rules.action_${r.action}`)}{/if}
                  {#if r.account_id} · {app.accounts.find((a) => a.id === r.account_id)?.email ?? ""}{/if}
                </div>
              </div>
              <Button size="icon-sm" variant="ghost" title={t("common.edit")} onclick={() => editRule(r)}><Pencil class="size-4" /></Button>
              <Button size="icon-sm" variant="ghost" onclick={() => deleteRule(r)}><Trash2 class="size-4" /></Button>
            </div>
          {:else}
            <div class="text-muted-foreground p-6 text-center">{t("rules.none")}</div>
          {/each}
        </div>
      {:else if tab === "categories"}
        <h3 class="text-lg font-semibold">{t("categories.title")}</h3>
        <div class="flex items-center gap-2">
          <Input class="w-60" placeholder={t("categories.name")} bind:value={newCatName} />
          <input type="color" bind:value={newCatColor} class="h-9 w-12" />
          <Button onclick={addCategory}><Plus class="size-4" />{t("common.add")}</Button>
        </div>
        <div class="divide-y rounded-md border">
          {#each app.categories as c (c.name)}
            <div class="flex items-center gap-3 p-2">
              <input type="color" value={c.color} class="h-7 w-10" onchange={async (e) => { await api.saveCategory(c.name, c.name, (e.target as HTMLInputElement).value); app.loadCategories(); }} />
              <span class="flex-1">{c.name}</span>
              <Button size="icon-sm" variant="ghost" onclick={() => renameCategory(c.name, c.color)}><Pencil class="size-4" /></Button>
              <Button size="icon-sm" variant="ghost" onclick={async () => { await api.deleteCategory(c.name); app.loadCategories(); }}><Trash2 class="size-4" /></Button>
            </div>
          {/each}
        </div>
      {:else if tab === "about"}
        <div class="flex items-center gap-4">
          <img src="/icon.svg" alt="" class="size-16" />
          <div>
            <h3 class="text-2xl font-semibold">Lynora</h3>
            <div class="text-muted-foreground">{t("about.tagline")}</div>
            <div class="text-muted-foreground text-xs">{t("about.version", info?.version ?? "")}</div>
          </div>
        </div>
        <p>{t("about.text")}</p>
        <div class="grid grid-cols-[180px_1fr] gap-2 text-xs">
          <span class="text-muted-foreground">{t("about.data_dir")}</span><span class="selectable font-mono">{info?.data_dir}</span>
          <span class="text-muted-foreground">{t("about.pst_support")}</span><span>{info?.readpst ? t("about.available") : t("about.pst_install")}</span>
        </div>
        <p class="text-muted-foreground text-xs">{t("about.license")}</p>
      {/if}
      </div>
    </div>
    <div class="flex shrink-0 justify-end gap-2 border-t px-4 py-3">
      <button data-default-button class="border-primary hover:bg-accent h-8 min-w-24 rounded border-2 px-4 text-[13px]" onclick={commit}>{t("common.ok")}</button>
      <button class="hover:bg-accent h-8 min-w-24 rounded border px-4 text-[13px]" onclick={cancel}>{t("common.cancel")}</button>
    </div>
{/snippet}

<RuleWizard
  bind:open={wizard.open}
  ruleId={wizard.id}
  accountId={wizard.account}
  name={wizard.name}
  enabled={wizard.enabled}
  prefillFrom={wizard.from}
  local={wizard.local}
  rule={wizard.rule}
  onsaved={async () => {
    rules = await api.listRules();
    if (!wizard.local) toast.success(t("rules.saved_server"));
  }}
/>
