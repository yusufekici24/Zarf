<script lang="ts">
  // Açılan ileti penceresi (Outlook'ta iletiye çift tıklayınca açılan pencere):
  // şerit + başlıklar + gövde. Yanıtlarken/iletirken Outlook'taki gibi bu pencere kapanır.
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import RibbonButton from "$lib/components/common/RibbonButton.svelte";
  import RibbonGroup from "$lib/components/ribbon/RibbonGroup.svelte";
  import QuickStepsGroup from "$lib/components/ribbon/QuickStepsGroup.svelte";
  import RibbonStack from "$lib/components/ribbon/RibbonStack.svelte";
  import Ribbon from "$lib/components/ribbon/Ribbon.svelte";
  import WindowTitleBar from "$lib/components/common/WindowTitleBar.svelte";
  import { windowFocus } from "$lib/stores/focus.svelte";
  import ReadingPane from "./ReadingPane.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { addDays, startOfDay, toTs } from "$lib/format";
  import type { MessageView } from "$lib/types";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick, untrack } from "svelte";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Archive from "@lucide/svelte/icons/archive";
  import Reply from "@lucide/svelte/icons/reply";
  import ReplyAll from "@lucide/svelte/icons/reply-all";
  import Forward from "@lucide/svelte/icons/forward";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import MailOpen from "@lucide/svelte/icons/mail-open";
  import Tag from "@lucide/svelte/icons/tag";
  import Flag from "@lucide/svelte/icons/flag";
  import Printer from "@lucide/svelte/icons/printer";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import ZoomOut from "@lucide/svelte/icons/zoom-out";
  import ZoomIn from "@lucide/svelte/icons/zoom-in";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import CalendarPlus from "@lucide/svelte/icons/calendar-plus";
  import MailCheck from "@lucide/svelte/icons/mail-check";
  import MailX from "@lucide/svelte/icons/mail-x";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Send from "@lucide/svelte/icons/send";
  import FileCog from "@lucide/svelte/icons/file-cog";
  import Backstage from "$lib/components/ribbon/Backstage.svelte";
  import BackstageTile from "$lib/components/ribbon/BackstageTile.svelte";
  import MessageProperties, { messageSource, sizeText } from "./MessageProperties.svelte";
  import { open as openDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";

  let { id, preload }: { id: number; preload?: MessageView } = $props();
  windowFocus.start();

  // Ana pencereden hazır gelen ileti ilk karede gösterilir (veritabanını beklemeden)
  let message = $state<MessageView | null>(untrack(() => (preload?.id === id ? preload : null)));
  let reading: ReadingPane | undefined = $state();
  /** Önceki/sonraki ileti için klasördeki sıra (tarih) */
  let siblings = $state<number[]>([]);

  async function load(mid: number) {
    try {
      message = await api.getMessage(mid);
      void getCurrentWindow().setTitle(`${message.subject || t("mail.no_subject")} - ${t("compose.window_msg")}`).catch(() => {});
      // Outlook: açılan ileti okundu sayılır
      if (!message.is_read && !message.external) {
        await api.setRead([mid], true);
        message = { ...message, is_read: true };
      }
    } catch (e) {
      app.error(e);
    }
  }

  /** Pencere ileti gövdesi çizilmeye hazır olunca gösterilir (SecondaryWindow bekler) */
  async function signalReady() {
    await tick();
    const f = document.querySelector<HTMLIFrameElement>("iframe");
    if (f && f.contentDocument?.readyState !== "complete") {
      await new Promise<void>((res) => {
        const t = setTimeout(res, 500);
        f.addEventListener("load", () => (clearTimeout(t), res()), { once: true });
      });
    }
    (window as unknown as { __contentReady?: boolean }).__contentReady = true;
    window.dispatchEvent(new Event("mailnix:content-ready"));
  }

  onMount(async () => {
    (window as unknown as { __contentReady?: boolean }).__contentReady = false;
    if (message) {
      // Hazır geldi: hemen gösterilir; başlık ve okundu bilgisi arkadan
      void signalReady();
      void getCurrentWindow().setTitle(`${message.subject || t("mail.no_subject")} - ${t("compose.window_msg")}`).catch(() => {});
      if (!message.is_read && !message.external) {
        void api.setRead([message.id], true).then(() => message && (message = { ...message, is_read: true })).catch(() => {});
      }
    } else {
      await load(id);
      void signalReady();
    }
    if (message && !message.external) {
      try {
        siblings = await api.siblingIds(message.folder_id);
      } catch {
        /* yoksay */
      }
    }
  });

  const has = $derived(!!message && !message.external);
  const pos = $derived(message ? siblings.indexOf(message.id) : -1);
  const today17 = () => toTs(startOfDay(new Date())) + 17 * 3600;
  const folders = $derived(app.folderGroups.flatMap((g) => g.folders.map((f) => ({ f, account: g.account?.email ?? "" }))));

  // ------------------------------------------------------------- Dosya görünümü
  let backstage = $state(false);
  let propsOpen = $state(false);
  let size = $state("");
  $effect(() => {
    if (!backstage || !message) return;
    const mid = message.id;
    void messageSource(mid).then((s) => {
      if (message?.id === mid && s) size = sizeText(new TextEncoder().encode(s).length);
    });
  });
  const currentFolder = $derived(message && !message.external ? folders.find((x) => x.f.id === message!.folder_id)?.f.display ?? "" : "");

  async function saveAllAttachments() {
    if (!message) return;
    const dir = await openDialog({ directory: true });
    if (dir && typeof dir === "string") toast.success(t("mail.attachments_saved", await api.saveAllAttachments(message.id, dir)));
  }

  /** Ayrı açılmış ileti dosyası (.msg/.eml) seçilen klasöre kopyalanır */
  async function toFolder(folderId: number, copy: boolean) {
    if (!message) return;
    if (message.external) {
      await api.importExternalMessage(message.id, folderId).catch((e) => app.error(e));
      toast.success(t("bs.copied"));
    } else if (copy) {
      await act(() => api.copyMessages([message!.id], folderId), false);
      toast.success(t("mail.copied", 1));
    }
    else await act(() => api.moveMessages([message!.id], folderId));
  }

  async function closeWindow() {
    await api.recycleWindow().catch(() => getCurrentWindow().destroy().catch(() => window.close()));
  }

  /** Yanıtla / Tümünü Yanıtla / İlet: aynı pencere yanıt yazma penceresine dönüşür
      (yeni pencere açılmaz; SecondaryWindow içeriği değiştirir) */
  function reply(mode: "reply" | "reply_all" | "forward") {
    if (!message) return;
    window.dispatchEvent(new CustomEvent("mailnix:become-compose", { detail: { mode, sourceId: message.id, key: Date.now() } }));
  }

  /** Yoksay (Outlook): bu konuşmanın sonraki iletileri doğrudan silinir, bu ileti de silinir */
  async function ignore() {
    const m = message;
    if (!m || m.external) return;
    const subj = m.subject.replace(/^(\s*(re|fw|fwd|ynt|ilt|ileti|yanıt|aw|wg|sv|tr|rv)\s*(\[\d+\])?\s*:\s*)+/i, "").trim();
    if (!subj) return;
    if (!(await app.confirm(t("ctx.ignore_title"), t("ctx.ignore_text", subj), { ok: t("ctx.ignore"), destructive: true }))) return;
    await act(async () => {
      await api.saveRule({ id: 0, name: t("ctx.ignore_rule", subj), enabled: true, account_id: m.account_id, field: "subject", op: "contains", value: subj, action: "delete", action_arg: "", stop: true });
      await api.deleteMessages([m.id]);
    });
  }

  /** Ek Olarak İlet: ileti .msg dosyası olarak yeni iletiye eklenir */
  async function forwardAsAttachment() {
    const m = message;
    if (!m) return;
    try {
      const f = await api.messageTempFile(m.id, "msg");
      app.compose({ mode: "new", data: { subject: `${t("compose.fw_prefix")}: ${m.subject}` }, attachments: [{ name: f.name, mime: f.mime, size: f.size, path: f.path }] });
    } catch (e) {
      app.error(e);
    }
  }

  async function act(fn: () => Promise<unknown>, close = true) {
    try {
      await fn();
      if (close) {
        // Outlook varsayılanı: silince/taşıyınca pencere kapanır (ayardan "sonrakini aç" seçilebilir)
        const next = siblings[pos + 1] ?? siblings[pos - 1];
        if (next != null && app.setting("open_next_after_delete", "0") === "1") {
          siblings = siblings.filter((x) => x !== message?.id);
          await load(next);
        } else await closeWindow();
      }
    } catch (e) {
      app.error(e);
    }
  }

  async function toggleCategory(name: string) {
    if (!message) return;
    await api.toggleCategory([message.id], name);
    await load(message.id);
  }

  function onkeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (e.key === "Escape" && !document.querySelector("[data-state=open][role=menu]") && !app.confirmReq) {
      e.preventDefault();
      void closeWindow();
    } else if (ctrl && e.shiftKey && k === "r") { e.preventDefault(); reply("reply_all"); }
    else if (ctrl && k === "r") { e.preventDefault(); reply("reply"); }
    else if (ctrl && k === "f") { e.preventDefault(); reply("forward"); }
    else if (ctrl && k === "p") { e.preventDefault(); reading?.print(); }
    else if (e.key === "Delete" && has) { e.preventDefault(); void act(() => api.deleteMessages([message!.id], e.shiftKey)); }
    else if (ctrl && k === "u" && has) { e.preventDefault(); void api.setRead([message!.id], false); }
    else if (ctrl && e.key === "." && pos >= 0 && siblings[pos + 1] != null) { e.preventDefault(); void load(siblings[pos + 1]); }
    else if (ctrl && e.key === "," && pos > 0) { e.preventDefault(); void load(siblings[pos - 1]); }
  }
</script>

<svelte:window {onkeydown} />

<div class="bg-background flex h-screen flex-col border {windowFocus.focused ? 'border-primary' : 'border-black/25 dark:border-white/20'}">
  <WindowTitleBar
    title={`${message?.subject || t("mail.no_subject")} - ${t("compose.window_msg")}`}
    onsave={message ? () => window.dispatchEvent(new CustomEvent("mailnix:save-as", { detail: message!.id })) : undefined}
    onprev={pos > 0 ? () => load(siblings[pos - 1]) : null}
    onnext={pos >= 0 && siblings[pos + 1] != null ? () => load(siblings[pos + 1]) : null}
    onclose={closeWindow}
  />
  <div class="relative flex min-h-0 flex-1 flex-col">
  {#if backstage && message}
    <Backstage
      items={[
        { key: "info", label: t("bs.info") },
        { key: "save_as", label: t("mail.save_as"), action: () => window.dispatchEvent(new CustomEvent("mailnix:save-as", { detail: message!.id })) },
        { key: "save_att", label: t("bs.save_attachments"), disabled: !message.attachments.some((a) => !a.inline), action: saveAllAttachments },
        { key: "print", label: t("common.print"), action: () => setTimeout(() => reading?.print(), 50) },
        { key: "close", label: t("common.close"), action: closeWindow },
      ]}
      bottom={[{ key: "options", label: t("bs.options"), action: () => app.openSettings("general") }]}
      onclose={() => (backstage = false)}
    >
      {#snippet children()}
        <h1 class="mb-5 truncate text-[22px] font-semibold">{message!.subject || t("mail.no_subject")}</h1>
        <BackstageTile icon={FolderInput} label={t("bs.move_to_folder")} title={t("bs.move_title")} text={t("bs.move_text")} details={currentFolder ? [t("bs.current_folder", currentFolder)] : []}>
          {#snippet menu()}
            <DropdownMenu.Sub>
              <DropdownMenu.SubTrigger>{t("mail.copy_to")}</DropdownMenu.SubTrigger>
              <DropdownMenu.SubContent class="max-h-96 w-64 overflow-y-auto">
                {#each folders as { f, account } (f.id)}
                  <DropdownMenu.Item onclick={() => toFolder(f.id, true)}><span class="truncate">{f.display}</span><span class="text-muted-foreground ml-auto truncate text-xs">{account}</span></DropdownMenu.Item>
                {/each}
              </DropdownMenu.SubContent>
            </DropdownMenu.Sub>
            <DropdownMenu.Separator />
            {#each folders as { f, account } (f.id)}
              <DropdownMenu.Item onclick={() => toFolder(f.id, false)}><span class="truncate">{f.display}</span><span class="text-muted-foreground ml-auto truncate text-xs">{account}</span></DropdownMenu.Item>
            {/each}
          {/snippet}
        </BackstageTile>
        <BackstageTile icon={Send} label={t("bs.resend")} title={t("bs.resend_title")} text={t("bs.resend_text")} disabled={message!.external} onclick={() => { app.compose({ mode: "edit", sourceId: message!.id }); backstage = false; }} />
        <BackstageTile icon={FileCog} label={t("bs.properties")} title={t("bs.properties")} text={t("bs.properties_text")} details={size ? [t("bs.size", size)] : []} onclick={() => (propsOpen = true)} />
      {/snippet}
    </Backstage>
    <MessageProperties message={message} bind:open={propsOpen} onchange={() => message && load(message.id)} />
  {/if}
  <div class="bg-chrome">
  <Ribbon tabs={[{ key: "message", label: t("ribbon.message") }]}>
    {#snippet file()}
      <button class="rounded-t px-2.5 pt-1 pb-1.5 hover:bg-black/5 dark:hover:bg-white/10" onclick={() => (backstage = true)}>{t("menu.file")}</button>
    {/snippet}
    {#snippet content()}
    <!-- Outlook ileti penceresi şeridi: Sil | Yanıtla | Taşı | Etiketler | Eylemler | Git -->
    <RibbonGroup label={t("ribbon.g_delete")}>
      <RibbonStack>
        <RibbonButton small icon={MailX} label={t("ctx.ignore")} disabled={!has} onclick={ignore} />
        <DropdownMenu.Root>
          <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} small icon={ShieldAlert} iconClass="text-[#c42b1c]" label={t("mail.junk")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
          <DropdownMenu.Content>
            <DropdownMenu.Item onclick={() => act(() => api.blockSender([message!.id]))}><ShieldAlert class="size-4 text-[#c42b1c]" />{t("mail.block_sender")}</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => act(() => api.moveToRole([message!.id], "junk"))}><MailX class="size-4" />{t("mail.mark_junk")}</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => act(() => api.moveToRole([message!.id], "inbox"))}><MailCheck class="size-4" />{t("mail.not_junk")}</DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      </RibbonStack>
      <RibbonButton icon={Trash2} label={t("common.delete")} disabled={!has} onclick={() => act(() => api.deleteMessages([message!.id]))} />
      <RibbonButton icon={Archive} iconClass="text-[#2f8f46]" label={t("mail.archive")} disabled={!has} onclick={() => act(() => api.moveToRole([message!.id], "archive"))} />
    </RibbonGroup>
    <RibbonGroup label={t("ribbon.g_respond")}>
      <RibbonButton icon={Reply} iconClass="text-[#7160e8]" label={t("mail.reply")} disabled={!message} onclick={() => reply("reply")} />
      <RibbonButton icon={ReplyAll} iconClass="text-[#7160e8]" label={t("mail.reply_all")} disabled={!message} onclick={() => reply("reply_all")} />
      <RibbonButton icon={Forward} iconClass="text-[#1f6fd1]" label={t("mail.forward")} disabled={!message} onclick={() => reply("forward")} />
      <RibbonStack>
        <RibbonButton small icon={CalendarPlus} iconClass="text-[#1f6fd1]" label={t("ribbon.meeting")} disabled={!has} onclick={() => window.dispatchEvent(new CustomEvent("mailnix:meeting-from-mail", { detail: message!.id }))} />
        <DropdownMenu.Root>
          <DropdownMenu.Trigger disabled={!message}>{#snippet child({ props })}<RibbonButton {...props} small icon={MessagesSquare} label={t("ribbon.more_respond")} menu disabled={!message} />{/snippet}</DropdownMenu.Trigger>
          <DropdownMenu.Content class="w-52">
            <DropdownMenu.Item onclick={forwardAsAttachment}><Paperclip class="size-4" />{t("mail.forward_attachment")}</DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      </RibbonStack>
    </RibbonGroup>
    <QuickStepsGroup disabled={!has} rows={() => (message && !message.external ? [message] : [])} ondone={(gone) => (gone ? void closeWindow() : message && void load(message.id))} />
    <RibbonGroup label={t("ribbon.g_move")}>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} icon={FolderInput} iconClass="text-[#1f6fd1]" label={t("mail.move")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
        <DropdownMenu.Content class="max-h-96 w-64 overflow-y-auto">
          {#each folders as { f, account } (f.id)}
            <DropdownMenu.Item onclick={() => act(() => api.moveMessages([message!.id], f.id))}><span class="truncate">{f.display}</span><span class="text-muted-foreground ml-auto truncate text-xs">{account}</span></DropdownMenu.Item>
          {/each}
        </DropdownMenu.Content>
      </DropdownMenu.Root>
      <RibbonStack>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} small icon={ListFilter} label={t("rules.title")} menu />{/snippet}</DropdownMenu.Trigger>
          <DropdownMenu.Content class="w-60">
            <DropdownMenu.Item disabled={!message || message.external} onclick={() => message && app.openRulesFor(message.sender)}>{t("rules.create_from")}</DropdownMenu.Item>
            <DropdownMenu.Item onclick={() => app.openSettings("rules")}>{t("rules.manage")}</DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      </RibbonStack>
    </RibbonGroup>
    <RibbonGroup label={t("ribbon.g_tags")}>
      <RibbonButton icon={MailOpen} label={t("mail.mark_unread")} disabled={!has} onclick={() => act(() => api.setRead([message!.id], false), false)} />
      <DropdownMenu.Root>
        <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} icon={Tag} iconClass="text-[#d97706]" label={t("mail.categorize")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
        <DropdownMenu.Content class="w-52">
          {#each app.categories as c (c.name)}
            <DropdownMenu.Item onclick={() => toggleCategory(c.name)}><span class="size-3 rounded-sm" style="background:{c.color}"></span>{c.name}</DropdownMenu.Item>
          {/each}
        </DropdownMenu.Content>
      </DropdownMenu.Root>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} icon={Flag} iconClass="text-red-600" label={t("mail.followup")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
        <DropdownMenu.Content class="w-52">
          <DropdownMenu.Item onclick={() => act(() => api.setFlagged([message!.id], true, today17()), false)}>{t("flag.today")}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => act(() => api.setFlagged([message!.id], true, today17() + 86400), false)}>{t("flag.tomorrow")}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => act(() => api.setFlagged([message!.id], true, toTs(addDays(startOfDay(new Date()), 8 - (new Date().getDay() || 7))) + 17 * 3600), false)}>{t("flag.next_week")}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => act(() => api.setFlagged([message!.id], true, null), false)}>{t("flag.no_date")}</DropdownMenu.Item>
          <DropdownMenu.Separator />
          <DropdownMenu.Item onclick={() => act(() => api.setFlagged([message!.id], false), false)}>{t("flag.complete")}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </RibbonGroup>
    <RibbonGroup label={t("ribbon.zoom")} last>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger disabled={!message}>{#snippet child({ props })}<RibbonButton {...props} icon={ZoomIn} label={t("ribbon.zoom")} disabled={!message} />{/snippet}</DropdownMenu.Trigger>
        <DropdownMenu.Content class="w-44">
          <DropdownMenu.Item onclick={() => reading?.zoomBy(0.1)}><ZoomIn class="size-4" />{t("view.zoom_in")}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => reading?.zoomBy(-0.1)}><ZoomOut class="size-4" />{t("view.zoom_out")}</DropdownMenu.Item>
          <DropdownMenu.Item onclick={() => reading?.zoomBy(0)}>{t("view.zoom_reset")}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </RibbonGroup>
    {/snippet}
  </Ribbon>
  </div>

  <div class="min-h-0 flex-1">
    <ReadingPane bind:this={reading} {message} standalone onreply={reply} />
  </div>
  </div>
</div>
