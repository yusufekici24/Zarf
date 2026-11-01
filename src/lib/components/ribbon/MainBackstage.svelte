<script lang="ts">
  // Ana pencerenin "Dosya" görünümü: Hesap Bilgileri, Aç ve Dışarı Aktar, kaydetme/yazdırma,
  // Seçenekler ve Çıkış (Outlook backstage düzeni).
  import Backstage from "./Backstage.svelte";
  import BackstageTile from "./BackstageTile.svelte";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import { Button } from "$lib/components/ui/button";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open as openDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { mail } from "$lib/stores/mail.svelte";
  import { t } from "$lib/i18n.svelte";
  import { importMail, importPst, openFile } from "$lib/actions";
  import Plus from "@lucide/svelte/icons/plus";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import UserCog from "@lucide/svelte/icons/user-cog";
  import MessageSquareReply from "@lucide/svelte/icons/message-square-reply";
  import Wrench from "@lucide/svelte/icons/wrench";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Database from "@lucide/svelte/icons/database";
  import MailPlus from "@lucide/svelte/icons/mail-plus";
  import CalendarPlus from "@lucide/svelte/icons/calendar-plus";
  import Contact from "@lucide/svelte/icons/contact";
  import Mail from "@lucide/svelte/icons/mail";
  import Server from "@lucide/svelte/icons/server";
  import BookUser from "@lucide/svelte/icons/book-user";
  import { fmtDateTime } from "$lib/format";

  let page = $state("info");
  let accountId = $state<number | null>(null);
  const account = $derived(app.accounts.find((a) => a.id === accountId) ?? app.accounts[0] ?? null);
  const group = $derived(app.folderGroups.find((g) => g.account?.id === account?.id));
  const roleFolder = (role: string) => group?.folders.find((f) => f.role === role);
  const kindLabel = (k: string) => (k === "exchange" ? "Microsoft Exchange" : "IMAP/SMTP");

  // Çevrimdışı adres defteri (Outlook OAB): durum ve elle güncelleme
  let abStatus = $state<{ synced: number; count: number; busy: boolean } | null>(null);
  let abBusy = $state(false);
  $effect(() => {
    if (app.backstage) void api.addressBookStatus().then((s) => (abStatus = s)).catch(() => {});
  });
  async function refreshAddressBook() {
    if (abBusy) return;
    abBusy = true;
    const id = toast.loading(t("ab.updating"), { description: t("ab.updating_text") });
    try {
      const n = await api.refreshAddressBook();
      toast.success(t("ab.updated"), { id, description: t("ab.updated_text", n ?? 0) });
    } catch (e) {
      toast.error(t("ab.failed"), { id, description: String(e) });
    } finally {
      abBusy = false;
      abStatus = await api.addressBookStatus().catch(() => abStatus);
    }
  }

  function close() {
    app.backstage = false;
  }

  async function emptyRole(role: string) {
    const f = roleFolder(role);
    if (!f) return;
    if (!(await app.confirm(t("folder.confirm_empty", f.display), t("folder.confirm_empty_text"), { ok: t("folder.empty"), destructive: true, safe: true }))) return;
    try {
      await api.emptyFolder(f.id);
      await app.loadFolders();
      app.bump("mail");
    } catch (e) {
      app.error(e);
    }
  }

  async function saveAllAttachments() {
    const m = mail.current;
    if (!m) return;
    const dir = await openDialog({ directory: true });
    if (dir && typeof dir === "string") toast.success(t("mail.attachments_saved", await api.saveAllAttachments(m.id, dir)));
  }
</script>

<Backstage
  bind:page
  items={[
    { key: "info", label: t("bs.info") },
    { key: "open", label: t("bs.open_export") },
    { key: "save_as", label: t("mail.save_as"), disabled: !mail.current, action: () => window.dispatchEvent(new CustomEvent("mailnix:save-as", { detail: mail.current?.id })) },
    { key: "save_att", label: t("bs.save_attachments"), disabled: !mail.current?.attachments?.some((a) => !a.inline), action: saveAllAttachments },
    { key: "print", label: t("common.print"), disabled: !mail.current, action: () => setTimeout(() => window.dispatchEvent(new Event("mailnix:print")), 50) },
  ]}
  bottom={[
    { key: "about", label: t("bs.about"), action: () => app.openSettings("about") },
    { key: "options", label: t("bs.options"), action: () => app.openSettings("general") },
    { key: "exit", label: t("menu.exit"), action: () => getCurrentWindow().close() },
  ]}
  onclose={close}
>
  {#snippet children(p)}
    {#if p === "info"}
      <h1 class="mb-4 text-[26px] font-light">{t("bs.account_info")}</h1>
      {#if account}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>
            {#snippet child({ props })}
              <button {...props} class="hover:bg-[#f3f2f1] dark:hover:bg-white/10 flex w-[440px] max-w-full items-center gap-3 rounded-[3px] border border-[#c8c6c4] px-3 py-1.5 text-left dark:border-white/25">
                <Mail class="text-primary size-6 shrink-0" strokeWidth={1.4} />
                <span class="min-w-0 flex-1"><span class="block truncate text-[14px]">{account.email}</span><span class="text-muted-foreground block text-[12px]">{kindLabel(account.kind)}</span></span>
                <ChevronDown class="size-4" />
              </button>
            {/snippet}
          </DropdownMenu.Trigger>
          <DropdownMenu.Content align="start" class="w-[440px]">
            {#each app.accounts as a (a.id)}
              <DropdownMenu.Item onclick={() => (accountId = a.id)}><Mail class="size-4" /><span class="truncate">{a.email}</span><span class="text-muted-foreground ml-auto text-xs">{kindLabel(a.kind)}</span></DropdownMenu.Item>
            {/each}
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      {/if}
      <Button variant="outline" size="sm" class="mt-2 mb-4 h-8 rounded-[3px]" onclick={() => { close(); app.openAccountDialog(); }}><Plus class="size-4" />{t("account.add")}</Button>

      <BackstageTile icon={UserCog} label={t("bs.account_settings")} title={t("bs.account_settings_title")} text={t("bs.account_settings_text")} details={account ? [account.kind === "exchange" ? account.ews_url || account.email : `${account.imap_host} · ${account.smtp_host}`] : []}>
        {#snippet menu()}
          <DropdownMenu.Item onclick={() => app.openSettings("accounts")}>{t("bs.account_settings")}…</DropdownMenu.Item>
          <DropdownMenu.Item disabled={!account} onclick={() => { close(); app.accountDialog = { open: true, account }; }}>{t("bs.change_account")}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => app.openSettings("signatures")}>{t("settings.tab_signatures")}</DropdownMenu.Item>
        {/snippet}
      </BackstageTile>
      <BackstageTile icon={MessageSquareReply} label={t("settings.tab_autoreply")} title={t("bs.autoreply_title")} text={t("bs.autoreply_text")} details={account?.auto_reply_enabled ? [t("bs.autoreply_on")] : []} iconClass="text-[#8e44ad]" onclick={() => app.openSettings("autoreply")} />
      <BackstageTile icon={Wrench} label={t("bs.tools")} title={t("bs.mailbox_title")} text={t("bs.mailbox_text")} iconClass="text-[#c25e00]">
        {#snippet menu()}
          <DropdownMenu.Item disabled={!roleFolder("trash")} onclick={() => emptyRole("trash")}>{t("bs.empty_trash")}</DropdownMenu.Item>
          <DropdownMenu.Item disabled={!roleFolder("junk")} onclick={() => emptyRole("junk")}>{t("bs.empty_junk")}</DropdownMenu.Item>
          <DropdownMenu.Separator />
          <DropdownMenu.Item disabled={!account} onclick={() => account && api.syncAccount(account.id)}>{t("bs.sync_account")}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => api.syncAll()}>{t("mail.send_receive")}<DropdownMenu.Shortcut>F9</DropdownMenu.Shortcut></DropdownMenu.Item>
        {/snippet}
      </BackstageTile>
      {#if app.accounts.some((a) => a.kind === "exchange")}
        <BackstageTile
          icon={BookUser}
          label={abBusy || abStatus?.busy ? t("ab.updating") : t("ab.update")}
          title={t("ab.title")}
          text={t("ab.text")}
          details={abStatus?.synced ? [t("ab.last", fmtDateTime(abStatus.synced), abStatus.count)] : [t("ab.never")]}
          iconClass="text-[#0f6cbd]"
          onclick={refreshAddressBook}
        />
      {/if}
      <BackstageTile icon={ListFilter} label={t("bs.manage_rules")} title={t("bs.rules_title")} text={t("bs.rules_text")} onclick={() => app.openSettings("rules")} />
      {#if account?.kind === "exchange"}
        <BackstageTile icon={Server} label={t("bs.server")} title={t("bs.server_title")} text={t("bs.server_text")} details={[account.ews_url || account.email]} onclick={() => { close(); app.accountDialog = { open: true, account }; }} />
      {/if}
    {:else if p === "open"}
      <h1 class="mb-5 text-[26px] font-light">{t("bs.open_export")}</h1>
      <BackstageTile icon={FolderOpen} label={t("bs.open_file")} title={t("bs.open_file_title")} text={t("bs.open_file_text")} iconClass="text-[#c19c00]" onclick={() => { close(); void openFile(); }} />
      <BackstageTile icon={Database} label={t("bs.import_pst")} title={t("bs.import_pst_title")} text={t("bs.import_pst_text")} onclick={() => { close(); void importPst(); }} />
      <BackstageTile icon={MailPlus} label={t("bs.import_mail")} title={t("bs.import_mail_title")} text={t("bs.import_mail_text")} onclick={() => { close(); void importMail(); }} />
      <BackstageTile icon={CalendarPlus} label={t("bs.import_calendar")} title={t("bs.import_calendar_title")} text={t("bs.import_calendar_text")} onclick={() => { close(); app.module = "calendar"; }} />
      <BackstageTile icon={Contact} label={t("bs.import_contacts")} title={t("bs.import_contacts_title")} text={t("bs.import_contacts_text")} onclick={() => { close(); app.module = "contacts"; }} />
    {/if}
  {/snippet}
</Backstage>
