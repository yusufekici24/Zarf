<script lang="ts">
  // Posta modülü: şerit + klasör bölmesi + ileti listesi + okuma bölmesi.
  import Splitter from "$lib/components/common/Splitter.svelte";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import RibbonButton from "$lib/components/common/RibbonButton.svelte";
  import QuickStepsGroup from "$lib/components/ribbon/QuickStepsGroup.svelte";
  import Ribbon from "$lib/components/ribbon/Ribbon.svelte";
  import RibbonGroup from "$lib/components/ribbon/RibbonGroup.svelte";
  import WifiOff from "@lucide/svelte/icons/wifi-off";
  import RibbonStack from "$lib/components/ribbon/RibbonStack.svelte";
  import FileMenu from "$lib/components/ribbon/FileMenu.svelte";
  import MailX from "@lucide/svelte/icons/mail-x";
  import MailCheck from "@lucide/svelte/icons/mail-check";
  import CalendarPlus from "@lucide/svelte/icons/calendar-plus";
  import Paperclip from "@lucide/svelte/icons/paperclip";
  import BookUser from "@lucide/svelte/icons/book-user";
  import AddressBookPicker from "$lib/components/common/AddressBookPicker.svelte";
  import Funnel from "@lucide/svelte/icons/funnel";
  import FolderSync from "@lucide/svelte/icons/folder-sync";
  import Send from "@lucide/svelte/icons/send";
  import Download from "@lucide/svelte/icons/download";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Eraser from "@lucide/svelte/icons/eraser";
  import Star from "@lucide/svelte/icons/star";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import CircleUser from "@lucide/svelte/icons/circle-user";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Info from "@lucide/svelte/icons/info";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import FolderPane from "./FolderPane.svelte";
  import MessageList from "./MessageList.svelte";
  import ReadingPane from "./ReadingPane.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { mail } from "$lib/stores/mail.svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import { addDays, startOfDay, toTs } from "$lib/format";
  import MailPlus from "@lucide/svelte/icons/mail-plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Archive from "@lucide/svelte/icons/archive";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import Reply from "@lucide/svelte/icons/reply";
  import ReplyAll from "@lucide/svelte/icons/reply-all";
  import Forward from "@lucide/svelte/icons/forward";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import MailOpen from "@lucide/svelte/icons/mail-open";
  import Tag from "@lucide/svelte/icons/tag";
  import Flag from "@lucide/svelte/icons/flag";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Printer from "@lucide/svelte/icons/printer";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
  import Plus from "@lucide/svelte/icons/plus";
  import Loader from "@lucide/svelte/icons/loader-circle";

  let reading: ReadingPane | undefined = $state();
  let ribbonTab = $state("home");
  /** Giriş > Bul > Adres Defteri (göz atma) */
  let bookOpen = $state(false);

  // Bölme boyutları (piksel, ayarlarda saklanır)
  let areaW = $state(1200);
  let areaH = $state(700);
  let folderW = $state(Number(app.setting("pane_folder_w", "240")) || 240);
  const showFolders = $derived(app.setting("show_folders", "1") === "1");
  /** Bölmelere kalan genişlik: kapsayıcının iç boşluğu (2×2px) ve ayırıcılar (6px) düşülür;
      yoksa liste en darken okuma bölmesi sağdan taşıyıp şeritle hizası bozuluyordu */
  const innerW = $derived(areaW - 4 - (showFolders ? 6 : 0) - (mail.readingPane === "right" ? 6 : 0));
  const folderEff = $derived(Math.max(150, Math.min(folderW, innerW - 260 - (mail.readingPane === "right" ? 300 : 0))));
  // Okuma bölmesi, liste ile paylaşılan alanın ORANI olarak saklanır: pencere büyüyünce
  // (ör. tam ekran) artan yer oranla dağılır ve en çok okuma bölmesi büyür; klasör bölmesi
  // sabit kalır. Eski sürümün piksel ayarı ilk ölçümde orana çevrilir.
  let ratioW = $state(Number(app.setting("pane_reading_ratio", "0")) || 0);
  let ratioH = $state(Number(app.setting("pane_reading_ratio_h", "0")) || 0);
  let readingW = $state(0);
  let readingH = $state(0);
  const shareW = $derived(Math.max(1, innerW - (showFolders ? folderEff : 0)));
  $effect(() => {
    const w = shareW;
    const h = areaH;
    untrack(() => {
      if (areaW <= 0) return;
      if (!ratioW) {
        const old = Number(app.setting("pane_reading_w", "0"));
        ratioW = old > 0 ? Math.min(0.75, Math.max(0.35, old / w)) : 0.58;
      }
      if (!ratioH && h > 0) {
        const old = Number(app.setting("pane_reading_h", "0"));
        ratioH = old > 0 ? Math.min(0.8, Math.max(0.3, old / h)) : 0.55;
      }
      readingW = Math.round(ratioW * w);
      readingH = Math.round(ratioH * h);
    });
  });
  function saveRatioW(v: number) {
    ratioW = Math.min(0.85, Math.max(0.2, v / shareW));
    void app.setSetting("pane_reading_ratio", ratioW.toFixed(4));
  }
  function saveRatioH(v: number) {
    ratioH = Math.min(0.85, Math.max(0.2, v / Math.max(1, areaH)));
    void app.setSetting("pane_reading_ratio_h", ratioH.toFixed(4));
  }
  const readingEff = $derived(Math.max(300, Math.min(readingW, innerW - (showFolders ? folderEff : 0) - 260)));
  const readingHEff = $derived(Math.max(140, Math.min(readingH, areaH - 160)));

  // Arama/filtre/sıralama değişince liste baştan; eşitleme olaylarında yerinde yenilenir
  $effect(() => {
    void app.search;
    void mail.filter;
    void mail.searchAll;
    void mail.sortBy;
    void mail.sortDesc;
    void mail.conversations;
    untrack(() => void mail.load());
  });
  let firstRev = true;
  $effect(() => {
    void app.revision.mail;
    if (firstRev) {
      firstRev = false;
      return;
    }
    untrack(() => mail.refresh());
  });

  // İlk açılışta (ya da seçili klasör artık görünmüyorsa) Gelen Kutusu'nu seç
  $effect(() => {
    if ((mail.folderId == null || !mail.folder()) && !mail.searchFolder && app.folderGroups.length) {
      const g = app.folderGroups.find((x) => x.account) ?? app.folderGroups[0];
      const inbox = g.folders.find((f) => f.role === "inbox") ?? g.folders[0];
      if (inbox) mail.selectFolder(inbox.id);
    }
  });

  const allFolders = $derived(app.folderGroups.flatMap((g) => g.folders.map((f) => ({ f, account: g.account?.email ?? t("folder.local") }))));
  const has = $derived(mail.selected.length > 0 || (!!mail.current && !mail.current.external));
  const pane = $derived(mail.readingPane);
  const today17 = () => toTs(startOfDay(new Date())) + 17 * 3600;

  const filters = [
    ["all", "filter.all"],
    ["unread", "filter.unread"],
    ["flagged", "filter.flagged"],
    ["attachments", "filter.attachments"],
    ["important", "filter.important"],
  ];

  export function printCurrent() {
    reading?.print();
  }

  import { onMount, untrack } from "svelte";
  onMount(() => {
    const h = () => reading?.print();
    window.addEventListener("lynora:print", h);
    return () => window.removeEventListener("lynora:print", h);
  });
</script>

<div class="flex h-full min-h-0 flex-col">
  <!-- Şerit (Outlook: Dosya | Giriş | Gönder/Al | Klasör | Görünüm | Yardım) -->
  <Ribbon tabs={[{ key: "home", label: t("ribbon.home") }, { key: "sendrecv", label: t("ribbon.send_receive") }, { key: "folder", label: t("ribbon.folder") }, { key: "view", label: t("ribbon.view") }, { key: "help", label: t("ribbon.help") }]} bind:active={ribbonTab}>
    {#snippet file()}<FileMenu />{/snippet}
    {#snippet content(tab)}
      {#if tab === "home"}
        <RibbonGroup label={t("ribbon.g_new")}>
          <RibbonButton icon={MailPlus} label={t("mail.new")} accent title={t("mail.new") + " (Ctrl+N)"} onclick={() => app.compose()} />
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={Plus} label={t("ribbon.new_items")} menu />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content>
              <DropdownMenu.Item onclick={() => app.compose()}>{t("item.mail")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.newItem("calendar")}>{t("item.event")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.newItem("calendar", { attendees: "" })}>{t("calendar.new_meeting")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.newItem("contacts")}>{t("item.contact")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.newItem("tasks")}>{t("item.task")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.newItem("notes")}>{t("item.note")}</DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_delete")}>
          <!-- Outlook: Yoksay ve Önemsiz ▾ (Göndereni Engelle, Önemsiz olarak işaretle, Önemsiz Değil) -->
          <RibbonStack>
            <RibbonButton small icon={MailX} label={t("ctx.ignore")} disabled={mail.selected.length !== 1} onclick={() => window.dispatchEvent(new Event("lynora:ignore"))} />
            <DropdownMenu.Root>
              <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} small icon={ShieldAlert} iconClass="text-[#c42b1c]" label={t("mail.junk")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
              <DropdownMenu.Content class="w-56">
                <DropdownMenu.Item onclick={() => mail.blockSender()}><ShieldAlert class="size-4 text-[#c42b1c]" />{t("mail.block_sender")}</DropdownMenu.Item>
                <DropdownMenu.Item onclick={() => mail.moveToRole("junk")}><MailX class="size-4" />{t("mail.mark_junk")}</DropdownMenu.Item>
                <DropdownMenu.Item onclick={() => mail.moveToRole("inbox")}><MailCheck class="size-4" />{t("mail.not_junk")}</DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </RibbonStack>
          <RibbonButton icon={Trash2} label={t("common.delete")} disabled={!has} onclick={() => mail.delete()} />
          <RibbonButton icon={Archive} iconClass="text-[#2f8f46]" label={t("mail.archive")} disabled={!has} onclick={() => mail.moveToRole("archive")} />
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_respond")}>
          <RibbonButton icon={Reply} iconClass="text-[#7160e8]" label={t("mail.reply")} disabled={!has} onclick={() => mail.reply("reply")} />
          <RibbonButton icon={ReplyAll} iconClass="text-[#7160e8]" label={t("mail.reply_all")} disabled={!has} onclick={() => mail.reply("reply_all")} />
          <RibbonButton icon={Forward} iconClass="text-[#1f6fd1]" label={t("mail.forward")} disabled={!has} onclick={() => mail.reply("forward")} />
          <RibbonStack>
            <RibbonButton small icon={CalendarPlus} iconClass="text-[#1f6fd1]" label={t("ribbon.meeting")} disabled={!has} onclick={() => window.dispatchEvent(new CustomEvent("lynora:meeting-from-mail", { detail: mail.ids()[0] }))} />
            <DropdownMenu.Root>
              <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} small icon={MessagesSquare} label={t("ribbon.more_respond")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
              <DropdownMenu.Content class="w-52">
                <DropdownMenu.Item onclick={() => mail.forwardAsAttachment()}><Paperclip class="size-4" />{t("mail.forward_attachment")}</DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </RibbonStack>
        </RibbonGroup>
        <QuickStepsGroup disabled={!has} rows={() => { const ids = mail.ids(); const r = mail.messages.filter((m) => ids.includes(m.id)); return r.length ? r : mail.current && ids.includes(mail.current.id) ? [mail.current] : []; }} ondone={() => void mail.load(true)} />
        <RibbonGroup label={t("ribbon.g_move")}>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} icon={FolderInput} iconClass="text-[#1f6fd1]" label={t("mail.move")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content class="max-h-96 w-64 overflow-y-auto">
              {#each allFolders as { f, account } (f.id)}
                <DropdownMenu.Item onclick={() => mail.move(f.id)}><span class="truncate">{f.display}</span><span class="text-muted-foreground ml-auto truncate text-xs">{account}</span></DropdownMenu.Item>
              {/each}
            </DropdownMenu.Content>
          </DropdownMenu.Root>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={ListFilter} label={t("rules.title")} menu />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content class="w-60">
              <DropdownMenu.Item disabled={!mail.current} onclick={() => mail.current && app.openRulesFor(mail.current.sender)}>{t("rules.create_from")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.openSettings("rules")}>{t("rules.manage")}</DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_tags")}>
          <RibbonButton icon={MailOpen} label={t("mail.read_unread")} disabled={!has} onclick={() => mail.toggleRead()} />
          <DropdownMenu.Root>
            <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} icon={Tag} iconClass="text-[#d97706]" label={t("mail.categorize")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content class="w-52">
              {#each app.categories as c (c.name)}
                <DropdownMenu.Item onclick={() => mail.toggleCategory(c.name)}><span class="size-3 rounded-sm" style="background:{c.color}"></span>{c.name}</DropdownMenu.Item>
              {/each}
              <DropdownMenu.Separator />
              <DropdownMenu.Item onclick={async () => { await api.setCategories(mail.ids(), []); mail.refresh(); }}>{t("mail.clear_categories")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => app.openSettings("categories")}>{t("mail.all_categories")}</DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
          <DropdownMenu.Root>
            <DropdownMenu.Trigger disabled={!has}>{#snippet child({ props })}<RibbonButton {...props} icon={Flag} iconClass="text-red-600" label={t("mail.followup")} menu disabled={!has} />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content class="w-52">
              <DropdownMenu.Item onclick={() => mail.flag(true, today17())}>{t("flag.today")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => mail.flag(true, today17() + 86400)}>{t("flag.tomorrow")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => mail.flag(true, toTs(addDays(startOfDay(new Date()), Math.max(0, 5 - new Date().getDay()))) + 17 * 3600)}>{t("flag.this_week")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => mail.flag(true, toTs(addDays(startOfDay(new Date()), 8 - (new Date().getDay() || 7))) + 17 * 3600)}>{t("flag.next_week")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => mail.flag(true, null)}>{t("flag.no_date")}</DropdownMenu.Item>
              <DropdownMenu.Separator />
              <DropdownMenu.Item onclick={() => mail.flag(false)}>{t("flag.complete")}</DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_find")} last>
          <RibbonStack>
            <RibbonButton small icon={BookUser} label={t("compose.address_book")} onclick={() => (bookOpen = true)} />
            <DropdownMenu.Root>
              <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} small icon={Funnel} label={t("ribbon.filter_mail")} menu />{/snippet}</DropdownMenu.Trigger>
              <DropdownMenu.Content class="w-52">
                {#each filters as [v, label] (v)}<DropdownMenu.CheckboxItem checked={mail.filter === v} onCheckedChange={() => (mail.filter = v)}>{t(label)}</DropdownMenu.CheckboxItem>{/each}
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </RibbonStack>
        </RibbonGroup>
      {:else if tab === "sendrecv"}
        <RibbonGroup label={t("ribbon.send_receive")}>
          <RibbonButton icon={RefreshCw} label={t("ribbon.send_receive_all")} title="F9" onclick={() => api.syncAll()} />
          <RibbonButton icon={FolderSync} label={t("ribbon.update_folder")} disabled={!mail.folderId} onclick={() => mail.folderId && api.syncFolder(mail.folderId)} />
          <RibbonButton icon={Send} label={t("ribbon.send_all")} onclick={() => api.processOutbox()} />
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_preferences")}>
          <RibbonButton icon={WifiOff} label={t("ribbon.work_offline")} active={app.net === "work_offline"} onclick={async () => { const off = app.net !== "work_offline"; await app.setSetting("work_offline", off ? "1" : "0"); app.net = off ? "work_offline" : "online"; }} />
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_download")} last>
          <RibbonButton icon={Download} label={t("mail.load_older")} disabled={!mail.canLoadOlder} onclick={() => mail.loadOlder()} />
        </RibbonGroup>
      {:else if tab === "folder"}
        {@const f = mail.folder()}
        <RibbonGroup label={t("ribbon.g_new")}>
          <RibbonButton icon={FolderPlus} label={t("folder.new")} onclick={() => window.dispatchEvent(new CustomEvent("lynora:new-folder", { detail: f?.id ?? null }))} />
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_actions")}>
          <RibbonButton icon={MailOpen} label={t("folder.mark_all_read")} disabled={!f} onclick={() => f && api.markFolderRead(f.id)} />
          <RibbonButton icon={Eraser} label={t("folder.empty")} disabled={!f} onclick={() => f && window.dispatchEvent(new CustomEvent("lynora:empty-folder", { detail: f.id }))} />
          <RibbonButton icon={ListFilter} label={t("rules.run_now")} disabled={!f} onclick={async () => f && app.info(t("rules.applied", await api.runRules(f.id)))} />
        </RibbonGroup>
        <RibbonGroup label={t("folder.favorites")} last>
          <RibbonButton icon={Star} label={f?.favorite ? t("folder.remove_favorite") : t("folder.add_favorite")} disabled={!f} onclick={async () => { if (f) { await api.setFolderFavorite(f.id, !f.favorite); await app.loadFolders(); } }} />
        </RibbonGroup>
      {:else if tab === "view"}
        <RibbonGroup label={t("ribbon.g_messages")}>
          <RibbonButton icon={MessagesSquare} label={t("view.conversations")} active={mail.conversations} onclick={() => app.setSetting("conversations", mail.conversations ? "0" : "1")} />
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={ArrowUpDown} label={t("view.sort")} menu />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content class="w-48">
              <DropdownMenu.RadioGroup value={mail.sortBy} onValueChange={(v) => (mail.sortBy = v as typeof mail.sortBy)}>
                <DropdownMenu.RadioItem value="date">{t("sort.date")}</DropdownMenu.RadioItem>
                <DropdownMenu.RadioItem value="from">{mail.sentLike ? t("sort.to") : t("sort.from")}</DropdownMenu.RadioItem>
                <DropdownMenu.RadioItem value="subject">{t("sort.subject")}</DropdownMenu.RadioItem>
                <DropdownMenu.RadioItem value="importance">{t("sort.importance")}</DropdownMenu.RadioItem>
                <DropdownMenu.RadioItem value="flag">{t("sort.flag")}</DropdownMenu.RadioItem>
              </DropdownMenu.RadioGroup>
              <DropdownMenu.Separator />
              <DropdownMenu.CheckboxItem checked={mail.sortDesc} onCheckedChange={(v) => (mail.sortDesc = !!v)}>{t("sort.descending")}</DropdownMenu.CheckboxItem>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
          <RibbonStack>
            <RibbonButton small icon={CalendarDays} label={t("view.group_by_date")} active={app.setting("group_by_date", "1") === "1"} onclick={() => app.setSetting("group_by_date", app.setting("group_by_date", "1") === "1" ? "0" : "1")} />
            <RibbonButton small icon={CircleUser} label={t("view.show_avatars")} active={app.setting("show_avatars", "1") === "1"} onclick={() => app.setSetting("show_avatars", app.setting("show_avatars", "1") === "1" ? "0" : "1")} />
          </RibbonStack>
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_filter")}>
          {#each filters as [v, label] (v)}
            {#if v !== "all"}<RibbonButton small icon={ListFilter} label={t(label)} active={mail.filter === v} onclick={() => (mail.filter = mail.filter === v ? "all" : v)} />{/if}
          {/each}
        </RibbonGroup>
        <RibbonGroup label={t("ribbon.g_layout")} last>
          <RibbonButton icon={PanelLeft} label={t("view.folder_pane")} active={showFolders} onclick={() => app.setSetting("show_folders", showFolders ? "0" : "1")} />
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>{#snippet child({ props })}<RibbonButton {...props} icon={PanelRight} label={t("view.reading_pane")} menu />{/snippet}</DropdownMenu.Trigger>
            <DropdownMenu.Content class="w-56">
              <DropdownMenu.RadioGroup value={pane} onValueChange={(v) => app.setSetting("reading_pane", v)}>
                <DropdownMenu.RadioItem value="right">{t("view.pane_right")}</DropdownMenu.RadioItem>
                <DropdownMenu.RadioItem value="bottom">{t("view.pane_bottom")}</DropdownMenu.RadioItem>
                <DropdownMenu.RadioItem value="off">{t("view.pane_off")}</DropdownMenu.RadioItem>
              </DropdownMenu.RadioGroup>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
          <RibbonButton icon={Settings2} label={t("settings.title")} onclick={() => app.openSettings("mail")} />
        </RibbonGroup>
      {:else if tab === "help"}
        <RibbonGroup label={t("ribbon.help")} last>
          <RibbonButton icon={Keyboard} label={t("help.shortcuts")} onclick={() => window.dispatchEvent(new Event("lynora:shortcuts"))} />
          <RibbonButton icon={Info} label={t("settings.tab_about")} onclick={() => app.openSettings("about")} />
        </RibbonGroup>
      {/if}
    {/snippet}
  </Ribbon>

  {#snippet listPane()}
    <div class="flex h-full min-h-0 flex-col">
      <!-- Outlook liste başlığı: Tümü / Okunmamış sekmeleri, sağda sıralama ve yön -->
      <div class="flex h-10 shrink-0 items-center gap-4 border-b px-4 text-[14px]">
        {#each [["all", "filter.all"], ["unread", "filter.unread"]] as [v, label] (v)}
          <button class="relative h-full {mail.filter === v ? 'text-primary font-semibold' : 'hover:text-primary'}" onclick={() => (mail.filter = v)}>
            {t(label)}
            {#if mail.filter === v}<span class="bg-primary absolute inset-x-0 bottom-0 h-[2px]"></span>{/if}
          </button>
        {/each}
        {#if !["all", "unread"].includes(mail.filter)}
          <span class="text-primary flex items-center gap-1 text-xs font-medium">{t(filters.find((f) => f[0] === mail.filter)?.[1] ?? "")}<button class="hover:bg-accent rounded" onclick={() => (mail.filter = "all")}>✕</button></span>
        {/if}
        <span class="flex-1"></span>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>{#snippet child({ props })}<button {...props} class="hover:bg-accent flex items-center gap-1 rounded px-1.5 py-0.5 text-[13px]">{t(`sort.by_${mail.sortBy}`)}<ChevronDown class="size-3.5" /></button>{/snippet}</DropdownMenu.Trigger>
          <DropdownMenu.Content class="w-48" align="end">
            <DropdownMenu.RadioGroup value={mail.sortBy} onValueChange={(v) => (mail.sortBy = v as typeof mail.sortBy)}>
              {#each ["date", "from", "subject", "importance", "flag"] as k (k)}<DropdownMenu.RadioItem value={k}>{t(`sort.by_${k}`)}</DropdownMenu.RadioItem>{/each}
            </DropdownMenu.RadioGroup>
            <DropdownMenu.Separator />
            {#each filters as [v, label] (v)}<DropdownMenu.CheckboxItem checked={mail.filter === v} onCheckedChange={() => (mail.filter = v)}>{t(label)}</DropdownMenu.CheckboxItem>{/each}
          </DropdownMenu.Content>
        </DropdownMenu.Root>
        <button class="hover:bg-accent rounded p-1" title={t("sort.descending")} onclick={() => (mail.sortDesc = !mail.sortDesc)}>
          {#if mail.sortDesc}<ArrowDown class="size-4" />{:else}<ArrowUp class="size-4" />{/if}
        </button>
      </div>
      {#if app.search}
        <label class="bg-muted/40 flex items-center gap-2 border-b px-3 py-1 text-xs">
          <input type="checkbox" bind:checked={mail.searchAll} />{t("mail.search_all_mailboxes")}
          {#if mail.serverSearching}<span class="text-muted-foreground ml-auto flex items-center gap-1"><Loader class="size-3 animate-spin" />{t("mail.searching_server")}</span>{/if}
        </label>
      {/if}
      <MessageList />
    </div>
  {/snippet}

  <!-- Bölmeler: klasör bölmesi piksel genişlikte; ileti listesi ve okuma bölmesi kalan alanı
       kayıtlı oranla paylaşır (pencere büyüyüp küçüldükçe oran korunur). -->
  <div class="flex min-h-0 flex-1 overflow-hidden px-0.5" bind:clientWidth={areaW} bind:clientHeight={areaH}>
    {#if showFolders}
      <div class="pane min-w-0 shrink-0" style="width:{folderEff}px">
        <FolderPane />
      </div>
      <Splitter bind:size={folderW} min={150} max={Math.max(160, innerW - 260 - (pane === "right" ? 300 : 0))} onend={(v) => app.setSetting("pane_folder_w", String(v))} />
    {/if}
    {#if pane === "bottom"}
      <div class="flex min-w-0 flex-1 flex-col">
        <div class="pane min-h-0 flex-1">{@render listPane()}</div>
        <Splitter direction="vertical" invert bind:size={readingH} min={140} max={Math.max(150, areaH - 160)} onend={saveRatioH} />
        <div class="pane min-h-0 shrink-0" style="height:{readingHEff}px"><ReadingPane bind:this={reading} message={mail.current} /></div>
      </div>
    {:else}
      <div class="pane min-w-[260px] flex-1">{@render listPane()}</div>
      {#if pane === "right"}
        <Splitter invert bind:size={readingW} min={300} max={Math.max(320, innerW - (showFolders ? folderEff : 0) - 260)} onend={saveRatioW} />
        <div class="pane min-w-0 shrink-0" style="width:{readingEff}px"><ReadingPane bind:this={reading} message={mail.current} /></div>
      {/if}
    {/if}
  </div>
</div>

<AddressBookPicker bind:open={bookOpen} browse />
