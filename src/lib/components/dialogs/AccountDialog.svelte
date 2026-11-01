<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { untrack } from "svelte";
  // E-posta hesabı ekleme / düzenleme.
  // Outlook gibi: oturum açmış kullanıcının adresini bulur, Autodiscover ile ayarları alır,
  // Kerberos bileti varsa parola sormadan (tek oturum açma) kurar.
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { api, errorText } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import type { Account, Discovery } from "$lib/types";
  import Loader from "@lucide/svelte/icons/loader-circle";
  import CheckCircle from "@lucide/svelte/icons/circle-check";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import Building2 from "@lucide/svelte/icons/building-2";
  import { toast } from "svelte-sonner";

  type Identity = Awaited<ReturnType<typeof api.systemIdentity>>;
  type Step = "email" | "working" | "done" | "form";

  let a = $state<Account>(blank());
  let password = $state("");
  let oauthToken = $state<string | null>(null);
  let step = $state<Step>("email");
  let advanced = $state(false);
  let testing = $state(false);
  let testLog = $state("");
  let note = $state("");
  let saving = $state(false);
  let identity = $state<Identity | null>(null);
  let progress = $state<string[]>([]);
  /** Otomatik kurulumda şu an yapılan adım */
  let currentStep = $state("");
  onMount(() => {
    let off: (() => void) | undefined;
    void listen<string>("account://progress", (e) => {
      currentStep = e.payload;
      progress = [...progress, e.payload];
    }).then((f) => (off = f)).catch(() => {});
    return () => off?.();
  });
  let needPassword = $state(false);
  let o365 = $state(false);
  let emailError = $state("");
  /** Kerberos olsa bile parola alanını göster (sunucu Kerberos'u kabul etmediğinde) */
  let askPassword = $state(false);

  function blank(): Account {
    return {
      id: 0, name: "", email: "", display_name: "", imap_host: "", imap_port: 993, imap_security: "ssl", smtp_host: "", smtp_port: 465, smtp_security: "ssl",
      username: "", signature: "", sync_limit: 500, auto_reply_enabled: false, auto_reply_text: "", kind: "imap", auth_method: "password", ews_url: "",
      caldav_url: "", carddav_url: "", auth_state: "ok", auth_error: "",
    };
  }

  const editing = $derived(a.id > 0);
  const paused = $derived(editing && app.accountDialog.account?.auth_state === "failed");
  const usesPassword = $derived(a.auth_method === "password" || a.auth_method === "ntlm");

  $effect(() => {
    if (!app.accountDialog.open) return;
    const acc = app.accountDialog.account;
    untrack(() => void init(acc));
  });

  async function init(acc: Account | null) {
    a = acc ? { ...acc } : blank();
    password = "";
    oauthToken = null;
    testLog = "";
    note = "";
    progress = [];
    needPassword = false;
    o365 = false;
    emailError = "";
    askPassword = false;
    advanced = !!acc;
    step = acc ? "form" : "email";
    try {
      identity = await api.systemIdentity();
    } catch {
      identity = null;
    }
    if (acc) return;
    if (identity) {
      // Yalnızca dizinden (AD) okunan gerçek adres yazılır; tahmin yazılmaz
      if (!a.email && identity.email_source === "directory") a.email = identity.email_guess;
      if (!a.display_name) a.display_name = identity.full_name || "";
      // Hiç hesap yokken ve kurumsal oturum (Kerberos) varsa hemen otomatik kur
      if (app.accountDialog.auto && identity.kerberos) void connect();
    }
  }

  function applyDiscovery(d: Discovery) {
    a.kind = d.kind || "imap";
    a.email = d.email || a.email;
    if (d.display_name && !a.display_name) a.display_name = d.display_name;
    a.ews_url = d.ews_url;
    if (d.imap_host) {
      a.imap_host = d.imap_host;
      a.imap_port = d.imap_port;
      a.imap_security = d.imap_security;
    }
    if (d.smtp_host) {
      a.smtp_host = d.smtp_host;
      a.smtp_port = d.smtp_port;
      a.smtp_security = d.smtp_security;
    }
    a.auth_method = d.auth_method || "password";
    if (d.username) a.username = d.username;
    if (!a.name) a.name = a.email;
    o365 = d.office365;
  }

  /** Outlook'taki "Bağlan": ayarları bul, sına, kaydet. */
  async function connect() {
    // E-posta boş olabilir: kullanıcı adından (ya da oturumdan) bulunur
    const canFind = !!(identity?.kerberos || a.username.trim() || (identity?.login && password));
    // Parola girildiyse Kerberos yerine parola ile denenir
    if (askPassword && !password) {
      emailError = t("account.signin_needed");
      return;
    }
    if (a.email.trim() && !a.email.includes("@")) {
      toast.error(t("account.invalid_email"));
      return;
    }
    if (!a.email.trim() && !canFind) {
      emailError = t("account.need_email");
      return;
    }
    emailError = "";
    step = "working";
    progress = [t("account.auto_searching", a.email || a.username || identity?.login || "")];
    try {
      const d = await api.discoverAccount(a.email.trim(), a.username.trim(), password);
      if (d.auth_failed) {
        // Tek denemede durulur; kullanıcı düzeltip yeniden dener
        step = "email";
        emailError = t("account.wrong_credentials");
        password = "";
        return;
      }
      if (d.need_credentials) {
        // Exchange bulundu; oturum için kullanıcı adı ve parola gerekiyor
        if (d.email) a.email = d.email;
        step = "email";
        askPassword = true;
        emailError = password ? t("account.wrong_credentials") : t("account.signin_needed");
        return;
      }
      if (d.need_email) {
        step = "email";
        emailError = t("account.need_email");
        return;
      }
      applyDiscovery(d);
      // Adımlar zaten canlı eklendi; yalnızca eksik kalanlar eklenir
      progress = [...progress, ...d.log.filter((l) => !progress.includes(l))];
      if (d.kind === "exchange") {
        progress = [...progress, t("account.auto_found_exchange")];
        if (d.office365) {
          // Microsoft 365: tarayıcıda kurumsal oturum açma
          step = "form";
          note = t("account.o365_note");
          return;
        }
      }
      // Kimlik bilgisi gerekmiyorsa (Kerberos) ya da parola verildiyse bağlantıyı sına
      if (a.auth_method === "kerberos" || password) {
        progress = [...progress, t("account.testing")];
        currentStep = t("account.step_testing");
        const r = await api.testAccount(a, password || null);
        progress = [...progress, r.log];
        if (r.imap_ok && (r.smtp_ok || a.kind === "exchange")) {
          await persist();
          step = "done";
          return;
        }
        if (a.auth_method === "kerberos") {
          // Kerberos başarısızsa parola ile devam (hesap kilitlenmesin diye tek deneme yapıldı)
          a.auth_method = "password";
        }
      }
      needPassword = true;
      step = "form";
      note = d.kind === "exchange" ? t("account.need_password_exchange") : d.source === "guess" ? t("account.check_servers") : "";
      advanced = d.source === "guess";
    } catch (e) {
      progress = [...progress, errorText(e)];
      step = "form";
      needPassword = true;
    }
  }

  async function oauthSignIn() {
    try {
      progress = [t("oauth.browser_opened")];
      testing = true;
      oauthToken = await api.oauthAuthorize("microsoft", a.kind === "exchange", a.email);
      a.auth_method = "oauth";
      toast.success(t("oauth.done"));
      await persist();
      step = "done";
    } catch (e) {
      app.error(e);
    } finally {
      testing = false;
    }
  }

  async function test() {
    testing = true;
    testLog = "";
    try {
      const r = await api.testAccount(a, password || null);
      testLog = r.log;
    } catch (e) {
      testLog = errorText(e);
    }
    testing = false;
  }

  async function persist() {
    if (!a.name) a.name = a.email;
    await api.saveAccount(a, password || null, oauthToken);
    await app.loadAccounts();
  }

  async function save() {
    if (!a.email.includes("@")) {
      toast.error(t("account.invalid_email"));
      return;
    }
    if (a.kind === "imap" && !a.imap_host) {
      const g = await api.guessServers(a.email);
      a.imap_host = String(g.imap_host);
      a.imap_port = Number(g.imap_port);
      a.imap_security = String(g.imap_security);
      a.smtp_host = String(g.smtp_host);
      a.smtp_port = Number(g.smtp_port);
      a.smtp_security = String(g.smtp_security);
    }
    saving = true;
    try {
      const wasPaused = paused;
      await persist();
      if (wasPaused && a.id) await api.syncAccount(a.id);
      close();
      toast.success(t("account.saved"));
    } catch (e) {
      app.error(e);
    }
    saving = false;
  }

  function close() {
    app.accountDialog = { open: false, account: null };
  }

  const SEC = [["ssl", "account.sec_ssl"], ["starttls", "account.sec_starttls"], ["none", "account.sec_none"]];
  const AUTH = [["password", "account.auth_password"], ["kerberos", "account.auth_kerberos"], ["ntlm", "account.auth_ntlm"], ["oauth", "account.auth_oauth"]];
</script>

<Dialog.Root open={app.accountDialog.open} onOpenChange={(o) => !o && close()}>
  <!-- Ayarlar penceresinin üstünde açılır (Hesaplar sekmesinden düzenleme / ekleme) -->
  <Dialog.Content class="z-[70] max-h-[92vh] overflow-y-auto sm:max-w-xl" overlayClass="z-[70]">
    <Dialog.Header>
      <Dialog.Title>{editing ? t("account.edit") : t("account.add")}</Dialog.Title>
      <Dialog.Description class={step === "form" ? "" : "sr-only"}>{step === "form" ? t("account.help") : t("account.auto_help")}</Dialog.Description>
    </Dialog.Header>

    {#if paused}
      <div class="flex gap-2 rounded-md bg-amber-50 p-3 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100">
        <KeyRound class="mt-0.5 size-4 shrink-0" />
        <div><div class="font-semibold">{t("account.paused_title", a.email)}</div><div>{t("account.paused_text")}</div>{#if a.auth_error}<div class="mt-1 opacity-80">{a.auth_error}</div>{/if}</div>
      </div>
    {/if}

    {#if step === "email"}
      <div class="grid gap-3 text-sm">
        <div class="grid gap-1.5">
          <Label>{t("account.email")}</Label>
          <Input type="email" bind:value={a.email} placeholder={t("account.email_auto")} onkeydown={(e) => e.key === "Enter" && connect()} />
        </div>
        <div class="grid gap-1.5">
          <Label>{t("account.username")}</Label>
          <Input bind:value={a.username} placeholder={identity?.login || t("account.username_hint")} autocomplete="username" onkeydown={(e) => e.key === "Enter" && connect()} />
        </div>
        {#if !identity?.kerberos || askPassword}
          <div class="grid gap-1.5">
            <Label>{t("account.password")}</Label>
            <Input type="password" bind:value={password} autocomplete="current-password" onkeydown={(e) => e.key === "Enter" && connect()} />
          </div>
        {/if}
        <div class="grid gap-1.5">
          <Label>{t("account.display_name")}</Label>
          <Input bind:value={a.display_name} placeholder={t("account.display_name_hint")} />
        </div>
        {#if emailError}<div class="text-destructive text-xs">{emailError}</div>{/if}
      </div>
      <Dialog.Footer class="sm:justify-between">
        <Button variant="link" class="px-0" onclick={() => { step = "form"; advanced = true; }}>{t("account.manual_setup")}</Button>
        <div class="flex gap-2">
          <Button variant="outline" onclick={close}>{t("common.cancel")}</Button>
          <Button onclick={connect}>{t("account.connect")}</Button>
        </div>
      </Dialog.Footer>
    {:else if step === "working"}
      <div class="flex flex-col items-center gap-3 py-6">
        <Loader class="text-primary size-8 animate-spin" />
        <div class="text-sm font-medium">{t("account.auto_working")}</div>
        <!-- Şu an yapılan adım (tek satır, değişerek) -->
        <div class="text-muted-foreground h-5 max-w-full truncate px-2 text-xs" title={currentStep}>{currentStep}</div>
      </div>
      <pre class="bg-muted max-h-40 overflow-auto rounded-md p-2 text-xs whitespace-pre-wrap">{progress.join("\n")}</pre>
    {:else if step === "done"}
      <div class="flex flex-col items-center gap-2 py-6 text-center">
        <CheckCircle class="size-10 text-emerald-600" />
        <div class="text-base font-semibold">{t("account.auto_done")}</div>
        <div class="text-muted-foreground text-sm">{a.email}</div>
      </div>
      <Dialog.Footer><Button onclick={close}>{t("account.auto_finish")}</Button></Dialog.Footer>
    {:else}
      <div class="grid grid-cols-[140px_1fr] items-center gap-2 text-sm">
        <Label>{t("account.email")}</Label>
        <Input type="email" bind:value={a.email} placeholder="ad@ornek.com" />
        <Label>{t("account.display_name")}</Label>
        <Input bind:value={a.display_name} placeholder={t("account.display_name_hint")} />
        <Label>{t("account.type")}</Label>
        <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={a.kind}>
          <option value="imap">IMAP / SMTP</option>
          <option value="exchange">Microsoft Exchange (EWS)</option>
        </select>
        <Label>{t("account.auth_method")}</Label>
        <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={a.auth_method}>
          {#each AUTH as [v, l] (v)}<option value={v}>{t(l)}</option>{/each}
        </select>
        {#if usesPassword}
          <Label>{t("account.username")}</Label>
          <Input bind:value={a.username} placeholder={identity?.login || (a.kind === "exchange" ? t("account.username_hint") : a.email)} autocomplete="username" />
          <Label>{t("account.password")}</Label>
          <Input type="password" bind:value={password} placeholder={editing && !paused ? t("account.password_keep") : ""} class={needPassword || paused ? "ring-primary ring-2" : ""} />
        {/if}
        <Label>{t("account.name")}</Label>
        <Input bind:value={a.name} placeholder={t("account.name_hint")} />
      </div>
      {#if a.auth_method === "oauth" || o365}
        <div class="flex items-center gap-2 rounded-md border p-2 text-xs">
          <Building2 class="size-4 shrink-0" /><span class="flex-1">{oauthToken ? t("oauth.done") : t("account.oauth_note")}</span>
          <Button size="sm" variant="outline" onclick={oauthSignIn} disabled={testing}>{#if testing}<Loader class="size-4 animate-spin" />{/if}{t("account.oauth_signin")}</Button>
        </div>
      {/if}
      {#if note}<div class="rounded-md bg-amber-50 p-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100">{note}</div>{/if}

      <button class="text-primary w-fit text-sm underline" onclick={() => (advanced = !advanced)}>{advanced ? t("account.hide_advanced") : t("account.show_advanced")}</button>

      {#if advanced}
        <div class="grid gap-3 rounded-md border p-3 text-sm">
          {#if a.kind === "exchange"}
            <div class="grid grid-cols-[140px_1fr] items-center gap-2">
              <Label>{t("account.ews_url")}</Label>
              <Input bind:value={a.ews_url} placeholder="https://mail.kurum.gov.tr/EWS/Exchange.asmx" />
            </div>
          {:else}
            <div class="font-semibold">{t("account.incoming")}</div>
            <div class="grid grid-cols-[1fr_90px_150px] gap-2">
              <Input bind:value={a.imap_host} placeholder="imap.ornek.com" />
              <Input type="number" bind:value={a.imap_port} />
              <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={a.imap_security} onchange={() => (a.imap_port = a.imap_security === "ssl" ? 993 : 143)}>
                {#each SEC as [v, l] (v)}<option value={v}>{t(l)}</option>{/each}
              </select>
            </div>
            <div class="font-semibold">{t("account.outgoing")}</div>
            <div class="grid grid-cols-[1fr_90px_150px] gap-2">
              <Input bind:value={a.smtp_host} placeholder="smtp.ornek.com" />
              <Input type="number" bind:value={a.smtp_port} />
              <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={a.smtp_security} onchange={() => (a.smtp_port = a.smtp_security === "ssl" ? 465 : a.smtp_security === "starttls" ? 587 : 25)}>
                {#each SEC as [v, l] (v)}<option value={v}>{t(l)}</option>{/each}
              </select>
            </div>
            <div class="font-semibold">{t("account.dav_title")}</div>
            <div class="grid grid-cols-[140px_1fr] items-center gap-2">
              <Label>CalDAV</Label>
              <Input bind:value={a.caldav_url} placeholder="https://bulut.ornek.com/remote.php/dav" />
              <Label>CardDAV</Label>
              <Input bind:value={a.carddav_url} placeholder="https://bulut.ornek.com/remote.php/dav" />
            </div>
            <p class="text-muted-foreground text-xs">{t("account.dav_hint")}</p>
          {/if}
          <div class="grid grid-cols-[140px_1fr] items-center gap-2">
            <Label>{t("account.sync_limit")}</Label>
            <Input type="number" min="0" bind:value={a.sync_limit} />
          </div>
        </div>
      {/if}

      {#if testLog || testing || progress.length}
        <pre class="bg-muted max-h-40 overflow-auto rounded-md p-2 text-xs whitespace-pre-wrap">{testing ? t("account.testing") : testLog || progress.join("\n")}</pre>
      {/if}

      <Dialog.Footer class="sm:justify-between">
        <div class="flex gap-2">
          <Button variant="outline" onclick={test} disabled={testing || (a.kind === "imap" ? !a.imap_host : !a.ews_url)}>{#if testing}<Loader class="size-4 animate-spin" />{/if}{t("account.test")}</Button>
          {#if !editing}<Button variant="ghost" onclick={connect}>{t("account.auto_retry")}</Button>{/if}
        </div>
        <div class="flex gap-2">
          <Button variant="outline" onclick={close}>{t("common.cancel")}</Button>
          <Button onclick={save} disabled={saving}>{t("common.save")}</Button>
        </div>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>
