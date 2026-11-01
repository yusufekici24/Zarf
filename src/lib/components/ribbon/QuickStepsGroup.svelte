<script lang="ts">
  // Şeritteki "Hızlı Adımlar" grubu (Outlook): iki sütunlu galeri, sayfa okları ve ▾ menüsü;
  // köşedeki ↘ düğmesi "Hızlı Adımları Yönet" penceresini açar. Ayarı eksik adıma ilk
  // tıklandığında "İlk Kullanım Ayarı" sorulur, sonra adım uygulanır.
  import * as Dialog from "$lib/components/ui/dialog";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import { Button } from "$lib/components/ui/button";
  import RibbonGroup from "$lib/components/ribbon/RibbonGroup.svelte";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import type { MessageRow } from "$lib/types";
  import { ACTION_TYPES, blankAction, incomplete, needsSetup, quickSteps, type QsAction, type QsActionType, type QuickStep } from "$lib/quicksteps.svelte";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import Forward from "@lucide/svelte/icons/forward";
  import Users from "@lucide/svelte/icons/users";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import MessageSquareReply from "@lucide/svelte/icons/message-square-reply";
  import Zap from "@lucide/svelte/icons/zap";
  import Flag from "@lucide/svelte/icons/flag";
  import Tag from "@lucide/svelte/icons/tag";
  import Mail from "@lucide/svelte/icons/mail";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Archive from "@lucide/svelte/icons/archive";
  import MailOpen from "@lucide/svelte/icons/mail-open";
  import CalendarPlus from "@lucide/svelte/icons/calendar-plus";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import X from "@lucide/svelte/icons/x";
  import Plus from "@lucide/svelte/icons/plus";
  import Copy from "@lucide/svelte/icons/copy";
  import Pencil from "@lucide/svelte/icons/pencil";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";

  let { rows, ondone, disabled = false }: { rows: () => MessageRow[]; ondone?: (gone: boolean) => void; disabled?: boolean } = $props();

  const ICONS = { move: FolderInput, forward: Forward, team: Users, done: CircleCheck, reply_delete: MessageSquareReply, new: Zap, flag: Flag, tag: Tag, mail: Mail, delete: Trash2, archive: Archive, read: MailOpen, meeting: CalendarPlus };
  const COLORS: Record<string, string> = { move: "text-[#1f6fd1]", forward: "text-[#1f6fd1]", team: "text-[#1f6fd1]", done: "text-[#2f8f46]", reply_delete: "text-[#7160e8]", new: "text-[#d97706]", flag: "text-red-600", tag: "text-[#d97706]", delete: "", archive: "text-[#2f8f46]", read: "", meeting: "text-[#1f6fd1]", mail: "" };
  type IconKey = keyof typeof ICONS;

  const steps = $derived(quickSteps.list);
  let page = $state(0);
  const pages = $derived(Math.max(1, Math.ceil(steps.length / 6)));
  const shown = $derived(steps.slice(page * 6, page * 6 + 6));
  $effect(() => {
    if (page >= pages) page = pages - 1;
  });

  // ------------------------------------------------------------ uygulama
  async function run(s: QuickStep) {
    if (needsSetup(s)) {
      openEditor(s, true);
      return;
    }
    const r = rows();
    if (!r.length && s.actions.some((a) => a.type !== "new_mail")) return;
    const gone = await quickSteps.run(s, r);
    ondone?.(gone);
  }

  // ------------------------------------------------------------ düzenleyici
  let editor = $state<{ open: boolean; step: QuickStep; first: boolean; isNew: boolean }>({ open: false, step: { id: "", name: "", icon: "new", tooltip: "", actions: [] }, first: false, isNew: false });
  let editorError = $state("");
  const folders = $derived(app.folderGroups.flatMap((g) => g.folders.map((f) => ({ id: f.id, label: `${g.account?.email ?? t("folder.local")} / ${f.display}` }))));

  function openEditor(s: QuickStep | null, first = false) {
    editorError = "";
    const step: QuickStep = s ? JSON.parse(JSON.stringify(s)) : { id: `qs${Date.now()}`, name: "", icon: "new", tooltip: "", actions: [blankAction("move")] };
    editor = { open: true, step, first, isNew: !s };
  }
  function newWith(type: QsActionType, icon: IconKey) {
    openEditor(null);
    editor.step.actions = [blankAction(type)];
    editor.step.icon = icon;
  }
  function setType(i: number, type: QsActionType) {
    editor.step.actions[i] = blankAction(type);
  }
  async function saveEditor() {
    const s = editor.step;
    if (!s.name.trim()) {
      editorError = t("qs.err_name");
      return;
    }
    if (!s.actions.length || s.actions.some(incomplete)) {
      editorError = t("qs.err_actions");
      return;
    }
    s.name = s.name.trim();
    await quickSteps.upsert(s);
    const runNow = editor.first;
    editor.open = false;
    if (runNow) await run(s);
  }

  // ------------------------------------------------------------ yönetim
  let manageOpen = $state(false);
  let picked = $state<string | null>(null);
  async function move(id: string, d: number) {
    const list = quickSteps.list;
    const i = list.findIndex((s) => s.id === id);
    const j = i + d;
    if (i < 0 || j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    await quickSteps.save(list);
  }
  async function remove(id: string) {
    const s = steps.find((x) => x.id === id);
    if (!s || !(await app.confirm(t("qs.delete_confirm", s.name), undefined, { ok: t("common.delete"), destructive: true }))) return;
    await quickSteps.save(quickSteps.list.filter((x) => x.id !== id));
    picked = null;
  }
  async function duplicate(id: string) {
    const s = steps.find((x) => x.id === id);
    if (!s) return;
    await quickSteps.upsert({ ...JSON.parse(JSON.stringify(s)), id: `qs${Date.now()}`, name: t("qs.copy_of", s.name) });
  }
  async function reset() {
    if (await app.confirm(t("qs.reset_confirm"), undefined, { ok: t("qs.reset") })) await quickSteps.reset();
  }

  const sel = "border-input bg-background h-8 rounded-md border px-2 text-[13px]";
</script>

{#snippet stepIcon(s: QuickStep, cls: string)}
  {@const Icon = ICONS[(s.icon as IconKey) ?? "new"] ?? Zap}
  <Icon class="{cls} {COLORS[s.icon] ?? ''}" />
{/snippet}

<RibbonGroup label={t("qs.title")} launcher={() => (manageOpen = true)} launcherTitle={t("qs.manage")}>
  <!-- Galeri: 2 sütun × 3 satır; sağda yukarı/aşağı ve tüm adımlar menüsü -->
  <div class="qs-gallery mt-0.5 flex h-[70px] items-stretch rounded-sm border bg-white dark:bg-transparent">
    <div class="qs-grid grid w-[244px] grid-flow-col grid-cols-2 grid-rows-3 gap-x-1 p-0.5">
      {#each shown as s (s.id)}
        <button class="ribbon-btn-sm min-w-0 justify-start" title={s.tooltip || s.name} disabled={disabled && !s.actions.every((a) => a.type === "new_mail")} onclick={() => run(s)}>
          {@render stepIcon(s, "size-4 shrink-0")}<span class="truncate">{s.name}</span>
        </button>
      {/each}
    </div>
    <div class="qs-side flex w-4 flex-col border-l">
      <button class="qs-pager hover:bg-accent flex flex-1 items-center justify-center disabled:opacity-30" disabled={page <= 0} title={t("qs.prev_row")} onclick={() => (page = Math.max(0, page - 1))}><ChevronUp class="size-3" /></button>
      <button class="qs-pager hover:bg-accent flex flex-1 items-center justify-center disabled:opacity-30" disabled={page >= pages - 1} title={t("qs.next_row")} onclick={() => (page = Math.min(pages - 1, page + 1))}><ChevronDown class="size-3" /></button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>{#snippet child({ props })}<button {...props} class="qs-more hover:bg-accent flex flex-1 items-center justify-center border-t" title={t("common.more")}><ChevronDown class="size-3" /><span class="sr-only">{t("common.more")}</span></button>{/snippet}</DropdownMenu.Trigger>
        <DropdownMenu.Content class="w-64" align="end">
          {#each steps as s (s.id)}
            <DropdownMenu.Item onclick={() => run(s)}>{@render stepIcon(s, "size-4")}{s.name}</DropdownMenu.Item>
          {/each}
          <DropdownMenu.Separator />
          <DropdownMenu.Sub>
            <DropdownMenu.SubTrigger><Zap class="size-4 text-[#d97706]" />{t("qs.new")}</DropdownMenu.SubTrigger>
            <DropdownMenu.SubContent class="w-56">
              <DropdownMenu.Item onclick={() => newWith("move", "move")}><FolderInput class="size-4" />{t("qs.n_move")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => newWith("categorize", "tag")}><Tag class="size-4" />{t("qs.n_categorize")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => newWith("flag", "flag")}><Flag class="size-4" />{t("qs.n_flag")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => newWith("new_mail", "mail")}><Mail class="size-4" />{t("qs.n_new_mail")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => newWith("forward", "forward")}><Forward class="size-4" />{t("qs.n_forward")}</DropdownMenu.Item>
              <DropdownMenu.Item onclick={() => newWith("meeting", "meeting")}><CalendarPlus class="size-4" />{t("qs.n_meeting")}</DropdownMenu.Item>
              <DropdownMenu.Separator />
              <DropdownMenu.Item onclick={() => openEditor(null)}>{t("qs.n_custom")}</DropdownMenu.Item>
            </DropdownMenu.SubContent>
          </DropdownMenu.Sub>
          <DropdownMenu.Item onclick={() => (manageOpen = true)}>{t("qs.manage")}</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </div>
  </div>
</RibbonGroup>

<!-- Hızlı adımı düzenle / İlk Kullanım Ayarı -->
<Dialog.Root bind:open={editor.open}>
  <Dialog.Content class="z-[70] sm:max-w-xl" overlayClass="z-[70]">
    <Dialog.Header>
      <Dialog.Title>{editor.first ? t("qs.first_use") : editor.isNew ? t("qs.new_title") : t("qs.edit_title")}</Dialog.Title>
      <Dialog.Description>{t("qs.editor_help")}</Dialog.Description>
    </Dialog.Header>
    <div class="grid gap-3 text-[13px]">
      <label class="grid grid-cols-[90px_1fr_auto] items-center gap-2">
        <span>{t("qs.name")}</span>
        <input class="{sel} w-full" bind:value={editor.step.name} placeholder={t("qs.name_ph")} />
        <select class={sel} bind:value={editor.step.icon} title={t("qs.icon")}>
          {#each Object.keys(ICONS) as k (k)}<option value={k}>{t(`qs.icon_${k}`)}</option>{/each}
        </select>
      </label>
      <div class="text-muted-foreground text-[12px] font-semibold uppercase">{t("qs.actions")}</div>
      <div class="grid max-h-80 gap-2 overflow-y-auto pr-1">
        {#each editor.step.actions as a, i (i)}
          <div class="bg-muted/40 grid gap-2 rounded-md border p-2">
            <div class="flex items-center gap-2">
              <select class="{sel} flex-1" value={a.type} onchange={(e) => setType(i, (e.target as HTMLSelectElement).value as QsActionType)}>
                {#each ACTION_TYPES as ty (ty)}<option value={ty}>{t(`qs.a_${ty}`)}</option>{/each}
              </select>
              <button class="hover:bg-accent rounded p-1" title={t("common.remove")} onclick={() => editor.step.actions.splice(i, 1)}><X class="size-4" /></button>
            </div>
            {#if a.type === "move" || a.type === "copy"}
              <select class="{sel} {a.folder == null ? 'border-amber-500' : ''}" value={a.folder ?? ""} onchange={(e) => { const v = (e.target as HTMLSelectElement).value; (a as Extract<QsAction, { type: "move" }>).folder = v ? Number(v) : null; }}>
                <option value="">{t("qs.choose_folder")}</option>
                {#each folders as f (f.id)}<option value={f.id}>{f.label}</option>{/each}
              </select>
            {:else if a.type === "flag"}
              <select class={sel} bind:value={a.when}>
                <option value="today">{t("flag.today")}</option>
                <option value="tomorrow">{t("flag.tomorrow")}</option>
                <option value="next_week">{t("flag.next_week")}</option>
                <option value="no_date">{t("flag.no_date")}</option>
              </select>
            {:else if a.type === "categorize"}
              <select class={sel} bind:value={a.name}>
                {#each app.categories as c (c.name)}<option value={c.name}>{c.name}</option>{/each}
              </select>
            {:else if a.type === "importance"}
              <select class={sel} value={String(a.level)} onchange={(e) => ((a as Extract<QsAction, { type: "importance" }>).level = Number((e.target as HTMLSelectElement).value))}>
                <option value="2">{t("importance.high")}</option>
                <option value="1">{t("importance.normal")}</option>
                <option value="0">{t("importance.low")}</option>
              </select>
            {:else if a.type === "forward" || a.type === "forward_attachment"}
              <input class="{sel} {!a.to.trim() ? 'border-amber-500' : ''}" bind:value={a.to} placeholder={t("qs.to_ph")} />
            {:else if a.type === "new_mail"}
              <input class="{sel} {!a.to.trim() ? 'border-amber-500' : ''}" bind:value={a.to} placeholder={t("qs.to_ph")} />
              <input class={sel} bind:value={a.cc} placeholder={t("qs.cc_ph")} />
              <input class={sel} bind:value={a.subject} placeholder={t("qs.subject_ph")} />
            {/if}
          </div>
        {/each}
      </div>
      <div>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>{#snippet child({ props })}<Button {...props} variant="outline" size="sm"><Plus class="size-4" />{t("qs.add_action")}</Button>{/snippet}</DropdownMenu.Trigger>
          <DropdownMenu.Content class="z-[80] max-h-80 w-60 overflow-y-auto">
            {#each ACTION_TYPES as ty (ty)}<DropdownMenu.Item onclick={() => editor.step.actions.push(blankAction(ty))}>{t(`qs.a_${ty}`)}</DropdownMenu.Item>{/each}
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      </div>
      <label class="grid grid-cols-[90px_1fr] items-center gap-2">
        <span>{t("qs.tooltip")}</span>
        <input class="{sel} w-full" bind:value={editor.step.tooltip} />
      </label>
      {#if editorError}<p class="text-destructive text-[12px]">{editorError}</p>{/if}
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (editor.open = false)}>{t("common.cancel")}</Button>
      <Button data-default-button onclick={saveEditor}>{editor.first ? t("qs.save_run") : t("common.save")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<!-- Hızlı Adımları Yönet -->
<Dialog.Root bind:open={manageOpen}>
  <Dialog.Content class="z-[60] sm:max-w-2xl" overlayClass="z-[60]">
    <Dialog.Header><Dialog.Title>{t("qs.manage_title")}</Dialog.Title></Dialog.Header>
    <div class="grid grid-cols-[1fr_200px] gap-3 text-[13px]">
      <div class="h-72 overflow-y-auto rounded-md border">
        {#each steps as s (s.id)}
          <button class="flex w-full items-center gap-2 px-3 py-1.5 text-left {picked === s.id ? 'bg-primary/12' : 'hover:bg-accent'}" onclick={() => (picked = s.id)} ondblclick={() => openEditor(s)}>
            {@render stepIcon(s, "size-4 shrink-0")}<span class="flex-1 truncate">{s.name}</span>
            {#if needsSetup(s)}<span class="text-[11px] text-amber-600">{t("qs.not_set")}</span>{/if}
          </button>
        {/each}
      </div>
      <div class="text-muted-foreground rounded-md border p-2 text-[12px]">
        {#if picked && steps.find((s) => s.id === picked)}
          {@const s = steps.find((x) => x.id === picked)!}
          <div class="text-foreground mb-1 font-semibold">{t("qs.actions")}</div>
          {#each s.actions as a, i (i)}<div>• {t(`qs.a_${a.type}`)}</div>{/each}
          {#if s.tooltip}<div class="mt-2 italic">{s.tooltip}</div>{/if}
        {:else}{t("qs.pick_hint")}{/if}
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <Button variant="outline" size="sm" disabled={!picked} onclick={() => { const s = steps.find((x) => x.id === picked); if (s) openEditor(s); }}><Pencil class="size-4" />{t("common.edit")}</Button>
      <Button variant="outline" size="sm" disabled={!picked} onclick={() => picked && duplicate(picked)}><Copy class="size-4" />{t("qs.duplicate")}</Button>
      <Button variant="outline" size="sm" disabled={!picked} onclick={() => picked && remove(picked)}><X class="size-4" />{t("common.delete")}</Button>
      <Button variant="outline" size="icon-sm" disabled={!picked} title={t("qs.up")} onclick={() => picked && move(picked, -1)}><ArrowUp class="size-4" /></Button>
      <Button variant="outline" size="icon-sm" disabled={!picked} title={t("qs.down")} onclick={() => picked && move(picked, 1)}><ArrowDown class="size-4" /></Button>
      <Button variant="outline" size="sm" onclick={() => openEditor(null)}><Plus class="size-4" />{t("qs.new")}</Button>
      <span class="flex-1"></span>
      <Button variant="ghost" size="sm" onclick={reset}>{t("qs.reset")}</Button>
    </div>
    <Dialog.Footer><Button data-default-button onclick={() => (manageOpen = false)}>{t("common.ok")}</Button></Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
