<script lang="ts">
  import Avatar from "$lib/components/common/Avatar.svelte";
  // Okuma bölmesi: başlıklar, bilgi çubukları, ekler ve gövde.
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import { Button } from "$lib/components/ui/button";
  import MessageBody from "./MessageBody.svelte";
  import MeetingRequest from "$lib/components/mail/MeetingRequest.svelte";
  import RecipientLine from "./RecipientLine.svelte";
  import ComposeWindow from "./ComposeWindow.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { untrack } from "svelte";
  import { mail } from "$lib/stores/mail.svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import { avatarColor, displayName, fmtDateTime, fmtListDate, fmtSize, initials, splitNameAddr } from "$lib/format";
  import type { MessageView } from "$lib/types";
  import Reply from "@lucide/svelte/icons/reply";
  import ReplyAll from "@lucide/svelte/icons/reply-all";
  import Forward from "@lucide/svelte/icons/forward";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import ImageOff from "@lucide/svelte/icons/image-off";
  import CalendarPlus from "@lucide/svelte/icons/calendar-plus";
  import Check from "@lucide/svelte/icons/check";
  import HelpCircle from "@lucide/svelte/icons/circle-help";
  import X from "@lucide/svelte/icons/x";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import { addTrustedSender, isTrustedSender } from "$lib/trust";
  import MailCheck from "@lucide/svelte/icons/mail-check";
  import Clock from "@lucide/svelte/icons/clock";
  import Flag from "@lucide/svelte/icons/flag";
  import AlertCircle from "@lucide/svelte/icons/circle-alert";
  import MailOpen from "@lucide/svelte/icons/mail-open";
  import Download from "@lucide/svelte/icons/download";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import { open as openDialog, save as saveDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";
  import { people } from "$lib/stores/people.svelte";
  import { i18n } from "$lib/i18n.svelte";

  /** Outlook başlık tarihi: "16.10.2026 Cum 21:19" */
  function fmtHeaderDate(ts: number): string {
    const d = new Date(ts * 1000);
    const day = d.toLocaleDateString(i18n.locale, { day: "2-digit", month: "2-digit", year: "numeric" });
    const wd = d.toLocaleDateString(i18n.locale, { weekday: "short" });
    const tm = d.toLocaleTimeString(i18n.locale, { hour: "2-digit", minute: "2-digit" });
    return `${day} ${wd} ${tm}`;
  }

  // Dar okuma bölmesinde düğmelerde yalnızca simge
  let paneW = $state(800);
  const narrow = $derived(paneW < 540);

  let {
    message,
    standalone = false,
    onreply,
  }: { message: MessageView | null; standalone?: boolean; onreply?: (mode: "reply" | "reply_all" | "forward") => void } = $props();

  function reply(mode: "reply" | "reply_all" | "forward", id: number) {
    if (onreply) onreply(mode);
    else mail.reply(mode, id);
  }

  let allowRemote = $state(false);

  async function respond(kind: "accept" | "tentative" | "decline") {
    if (!message) return;
    try {
      await api.respondMeeting(message.id, kind);
      toast.success(t(`meeting.${kind}_done`));
      app.bump("calendar");
    } catch (e) {
      app.error(e);
    }
  }
  // Varsayılan yakınlaştırma: Ayarlar > Posta > Okuma bölmesi
  const baseZoom = $derived(Math.min(2.5, Math.max(0.5, Number(app.setting("reading_zoom", "100")) / 100 || 1)));
  let zoom = $state(untrack(() => baseZoom));
  $effect(() => {
    zoom = baseZoom;
  });
  let body: MessageBody | undefined = $state();
  let trusted = $state(false);

  $effect(() => {
    const m = message;
    allowRemote = app.setting("load_remote", "0") === "1";
    trusted = false;
    if (m && !allowRemote) {
      const addr = splitNameAddr(m.sender).addr;
      if (isTrustedSender(addr)) trusted = true;
      else if (addr) api.contactByEmail(addr).then((c) => { if (c && message?.id === m.id) trusted = true; });
    }
  });

  function trustSender() {
    const addr = message ? splitNameAddr(message.sender).addr : "";
    if (addr) void addTrustedSender(addr);
  }

  const pending = $derived.by(() => {
    if (!message?.pending?.startsWith("send:")) return null;
    const [, at] = message.pending.split(":");
    return { at: Number(at), failed: message.snippet.startsWith("[!]") };
  });

  // Çift tıklama (Outlook alışkanlığı) eki iki kez açmasın
  let lastOpen = { i: -1, at: 0 };
  async function openAttachment(i: number, name: string) {
    if (lastOpen.i === i && Date.now() - lastOpen.at < 1500) return;
    lastOpen = { i, at: Date.now() };
    if (!message) return;
    const lower = name.toLowerCase();
    try {
      if (lower.endsWith(".eml") || lower.endsWith(".msg")) {
        app.openMessage(await api.openAttachmentAsMessage(message.id, i), name);
        return;
      }
      if (lower.endsWith(".ics")) {
        toast.success(t("import.events_done", await api.importAttachmentCalendar(message.id)));
        app.bump("calendar");
        return;
      }
      await api.openAttachment(await api.attachmentTempPath(message.id, i));
    } catch (e) {
      app.error(e);
    }
  }

  async function saveAttachment(i: number, name: string) {
    if (!message) return;
    const path = await saveDialog({ defaultPath: name });
    if (path) await api.saveAttachment(message.id, i, path);
  }

  async function saveAll() {
    if (!message) return;
    const dir = await openDialog({ directory: true });
    if (dir && typeof dir === "string") toast.success(t("mail.attachments_saved", await api.saveAllAttachments(message.id, dir)));
  }

  export function print() {
    body?.print();
  }

  /** Yakınlaştır (şerit): adım +/-, 0 = %100 */
  export function zoomBy(step: number) {
    zoom = step === 0 ? baseZoom : Math.min(2.5, Math.max(0.5, Math.round((zoom + step) * 10) / 10));
  }
</script>

{#if !standalone && mail.inlineReply}
  {#key mail.inlineReply.key}
    <ComposeWindow req={mail.inlineReply} inline onclosed={() => (mail.inlineReply = null)} />
  {/key}
{:else if !message}
  <div class="text-muted-foreground flex h-full flex-col items-center justify-center gap-2 text-sm">
    <MailOpen class="size-12 opacity-30" />
    {mail.selected.length > 1 ? t("mail.n_selected", mail.selected.length) : t("mail.select_to_read")}
  </div>
{:else}
  {@const m = message}
  {@const sender = splitNameAddr(m.sender)}
  {@const senderName = (sender.name && !sender.name.includes("@") ? sender.name : "") || (sender.addr && people.name(sender.addr)) || sender.addr || m.sender}
  {@const cats = m.categories ? m.categories.split(",").filter(Boolean) : []}
  <div class="flex h-full min-h-0 flex-col" bind:clientWidth={paneW}>
    <div class="space-y-2 border-b px-4 pt-3 pb-2">
      <div class="flex items-start gap-2">
        <h2 class="selectable flex-1 text-lg leading-tight font-semibold">{m.subject || t("mail.no_subject")}</h2>
        {#if m.is_flagged}<Flag class="mt-1 size-4 fill-red-600 text-red-600" />{/if}
        {#if m.importance === 2}<AlertCircle class="mt-1 size-4 text-red-600" />{/if}
      </div>
      {#if cats.length}
        <div class="flex flex-wrap gap-1">
          {#each cats as c (c)}<span class="rounded px-2 py-0.5 text-xs text-white" style="background:{app.categoryColor(c)}">{c}</span>{/each}
        </div>
      {/if}
      <!-- Outlook: gönderen ve alıcılar solda; sağda çerçeveli Yanıtla | Tümünü Yanıtla | İlet ve "..."; altında tarih -->
      <div class="flex items-start gap-3">
        <button data-card-trigger class="shrink-0 rounded-full" onclick={(e) => people.openCard(m.sender, e)}>
          <Avatar name={senderName} email={sender.addr} class="size-10 text-sm" />
        </button>
        <div class="selectable min-w-0 flex-1 text-sm">
          <div class="flex items-start gap-2">
            <button data-card-trigger class="min-w-0 truncate text-left text-[15px] hover:underline" title={sender.addr} onclick={(e) => people.openCard(m.sender, e)}>
              {senderName}{#if sender.addr && senderName !== sender.addr}<span class="text-muted-foreground">&nbsp;&lt;{sender.addr}&gt;</span>{/if}
            </button>
            <span class="flex-1"></span>
            <div class="flex shrink-0 items-center gap-1.5">
              <div class="flex h-8 items-stretch overflow-hidden rounded-md border text-[13px]">
                <button class="hover:bg-accent flex items-center gap-1.5 px-2.5" onclick={() => reply("reply", m.id)} title={t("mail.reply")}><Reply class="size-4 text-violet-700 dark:text-violet-300" />{#if !narrow}{t("mail.reply")}{/if}</button>
                <button class="hover:bg-accent flex items-center gap-1.5 border-l px-2.5" onclick={() => reply("reply_all", m.id)} title={t("mail.reply_all")}><ReplyAll class="size-4 text-violet-700 dark:text-violet-300" />{#if !narrow}{t("mail.reply_all")}{/if}</button>
                <button class="hover:bg-accent flex items-center gap-1.5 border-l px-2.5" onclick={() => reply("forward", m.id)} title={t("mail.forward")}><Forward class="size-4 text-sky-700 dark:text-sky-300" />{#if !narrow}{t("mail.forward")}{/if}</button>
              </div>
              <DropdownMenu.Root>
                <DropdownMenu.Trigger>
                  {#snippet child({ props })}<button {...props} class="hover:bg-accent flex h-8 w-8 items-center justify-center rounded-md border"><Ellipsis class="size-4" /></button>{/snippet}
                </DropdownMenu.Trigger>
                <DropdownMenu.Content align="end" class="w-56">
                  <DropdownMenu.Item onclick={() => print()}>{t("common.print")}</DropdownMenu.Item>
                  <DropdownMenu.Item onclick={() => window.dispatchEvent(new CustomEvent("lynora:save-as", { detail: m.id }))}>{t("mail.save_as")}</DropdownMenu.Item>
                  <DropdownMenu.Item onclick={() => (zoom = Math.min(2.5, zoom + 0.1))}><ZoomIn class="size-4" />{t("view.zoom_in")}</DropdownMenu.Item>
                  <DropdownMenu.Item onclick={() => (zoom = Math.max(0.5, zoom - 0.1))}><ZoomOut class="size-4" />{t("view.zoom_out")}</DropdownMenu.Item>
                  <DropdownMenu.Item onclick={() => (zoom = baseZoom)}>{t("view.zoom_reset")}</DropdownMenu.Item>
                  {#if !m.external}
                    <DropdownMenu.Separator />
                    <DropdownMenu.Item onclick={() => window.dispatchEvent(new CustomEvent("lynora:add-contact", { detail: m.sender }))}>{t("mail.add_sender_contact")}</DropdownMenu.Item>
                    <DropdownMenu.Item onclick={() => window.dispatchEvent(new CustomEvent("lynora:meeting-from-mail", { detail: m.id }))}>{t("mail.reply_meeting")}</DropdownMenu.Item>
                    <DropdownMenu.Item onclick={() => app.openRulesFor(m.sender)}>{t("rules.create_from")}</DropdownMenu.Item>
                    <DropdownMenu.Item onclick={() => window.dispatchEvent(new CustomEvent("lynora:view-source", { detail: m.id }))}>{t("mail.view_source")}</DropdownMenu.Item>
                  {/if}
                </DropdownMenu.Content>
              </DropdownMenu.Root>
            </div>
          </div>
          <div class="flex items-start gap-2">
            <div class="min-w-0 flex-1">
              {#key m.id}
                <RecipientLine label={t("mail.to")} value={m.to_addr} />
                {#if m.cc}<RecipientLine label={t("mail.cc")} value={m.cc} />{/if}
                {#if m.bcc}<RecipientLine label={t("mail.bcc")} value={m.bcc} />{/if}
              {/key}
            </div>
            <div class="text-muted-foreground shrink-0 pt-0.5 text-xs">{fmtHeaderDate(m.date)}</div>
          </div>
        </div>
      </div>

      {#if m.has_remote && !allowRemote && !trusted}
        <div class="bg-muted flex items-center gap-2 rounded-md px-3 py-1.5 text-xs">
          <ImageOff class="size-4 shrink-0" />
          <span class="flex-1">{t("mail.remote_blocked")}</span>
          <Button size="sm" variant="outline" class="h-6 text-xs" onclick={() => (allowRemote = true)}>{t("mail.load_images")}</Button>
          <Button size="sm" variant="outline" class="h-6 text-xs" onclick={trustSender}>{t("mail.trust_sender")}</Button>
        </div>
      {/if}
      {#if m.has_calendar}
        <MeetingRequest id={m.id} organizerView={mail.sentLike}>
          {#snippet fallback()}
            <div class="flex items-center gap-2 rounded-md bg-blue-50 px-3 py-1.5 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">
              <CalendarPlus class="size-4 shrink-0" />
              <span class="flex-1">{t("mail.has_invite")}</span>
              {#if !mail.sentLike}
                <Button size="sm" variant="outline" class="h-6 text-xs" onclick={() => respond("accept")}><Check class="size-3.5 text-emerald-600" />{t("meeting.accept")}</Button>
                <Button size="sm" variant="outline" class="h-6 text-xs" onclick={() => respond("tentative")}><HelpCircle class="size-3.5 text-violet-600" />{t("meeting.tentative")}</Button>
                <Button size="sm" variant="outline" class="h-6 text-xs" onclick={() => respond("decline")}><X class="size-3.5 text-red-600" />{t("meeting.decline")}</Button>
              {/if}
              <Button size="sm" variant="ghost" class="h-6 text-xs" onclick={async () => { toast.success(t("import.events_done", await api.importAttachmentCalendar(m.id))); app.bump("calendar"); }}>{t("mail.add_to_calendar")}</Button>
            </div>
          {/snippet}
        </MeetingRequest>
      {/if}
      {#if m.has_vcard}
        <div class="bg-muted flex items-center gap-2 rounded-md px-3 py-1.5 text-xs">
          <UserPlus class="size-4 shrink-0" />
          <span class="flex-1">{t("mail.has_vcard")}</span>
          <Button size="sm" variant="outline" class="h-6 text-xs" onclick={async () => { toast.success(t("import.contacts_done", await api.importAttachmentVcard(m.id))); app.bump("contacts"); }}>{t("mail.add_contacts")}</Button>
        </div>
      {/if}
      {#if m.read_receipt_to && !m.is_answered && !m.external}
        <div class="bg-muted flex items-center gap-2 rounded-md px-3 py-1.5 text-xs">
          <MailCheck class="size-4 shrink-0" />
          <span class="flex-1">{t("mail.receipt_requested")}</span>
          <Button size="sm" variant="outline" class="h-6 text-xs" onclick={() => api.sendReadReceipt(m.id).then(() => toast.success(t("mail.receipt_sent")))}>{t("mail.send_receipt")}</Button>
        </div>
      {/if}
      {#if pending}
        <div class="flex items-center gap-2 rounded-md px-3 py-1.5 text-xs {pending.failed ? 'bg-red-50 text-red-900 dark:bg-red-950 dark:text-red-100' : 'bg-amber-50 text-amber-900 dark:bg-amber-950 dark:text-amber-100'}">
          <Clock class="size-4 shrink-0" />
          <span class="flex-1">{pending.failed ? m.snippet.slice(4) : pending.at > Date.now() / 1000 ? t("mail.scheduled", fmtDateTime(pending.at)) : t("mail.in_outbox")}</span>
          <Button size="sm" variant="outline" class="h-6 text-xs" onclick={() => api.retryOutbox(m.id)}>{t("mail.send_now")}</Button>
        </div>
      {/if}
      {#if m.external}
        <div class="bg-muted flex items-center gap-2 rounded-md px-3 py-1.5 text-xs">
          <span class="flex-1">{t("mail.external_file")}</span>
          <Button size="sm" variant="outline" class="h-6 text-xs" onclick={() => api.importExternalMessage(m.id).then(() => toast.success(t("mail.imported_inbox")))}>{t("mail.import_inbox")}</Button>
        </div>
      {/if}

      {#if m.attachments.length}
        <div class="flex flex-wrap items-center gap-1.5">
          {#each m.attachments as a (a.index)}
            <DropdownMenu.Root>
              <div class="hover:bg-accent flex items-center rounded-md border text-xs">
                <button class="flex items-center gap-1.5 py-1 pr-1 pl-2" onclick={() => openAttachment(a.index, a.name)} title={t("mail.open_attachment")}>
                  <Paperclip class="size-3.5" /><span class="max-w-48 truncate">{a.name}</span><span class="text-muted-foreground">{fmtSize(a.size)}</span>
                </button>
                <DropdownMenu.Trigger class="border-l px-1 py-1">▾</DropdownMenu.Trigger>
              </div>
              <DropdownMenu.Content>
                <DropdownMenu.Item onclick={() => openAttachment(a.index, a.name)}>{t("common.open")}</DropdownMenu.Item>
                <DropdownMenu.Item onclick={() => saveAttachment(a.index, a.name)}>{t("mail.save_attachment")}</DropdownMenu.Item>
                <DropdownMenu.Item onclick={saveAll}>{t("mail.save_all")}</DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          {/each}
          {#if m.attachments.length > 1}
            <Button variant="ghost" size="sm" class="h-7 text-xs" onclick={saveAll}><Download class="size-3.5" />{t("mail.save_all")}</Button>
          {/if}
        </div>
      {/if}
    </div>

    {#if mail.thread.length > 1 && !standalone}
      <div class="flex gap-1 overflow-x-auto border-b px-3 py-1.5">
        {#each mail.thread as tm (tm.id)}
          <button class="shrink-0 rounded border px-2 py-0.5 text-xs {tm.id === m.id ? 'bg-primary text-primary-foreground' : 'hover:bg-accent'}" onclick={() => mail.showOnly(tm.id)}>
            {displayName(tm.sender)} · {fmtListDate(tm.date)}
          </button>
        {/each}
      </div>
    {/if}
    <div class="min-h-0 flex-1">
      <MessageBody bind:this={body} html={m.html} text={m.text} allowRemote={allowRemote || trusted} {zoom} />
    </div>
  </div>
{/if}
