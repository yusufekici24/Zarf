<script lang="ts">
  import { untrack } from "svelte";
  import Ribbon from "$lib/components/ribbon/Ribbon.svelte";
  import FileMenu from "$lib/components/ribbon/FileMenu.svelte";
  import Info from "@lucide/svelte/icons/info";
  // Görevler modülü (Outlook Görevler / To Do).
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Textarea } from "$lib/components/ui/textarea";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import RibbonButton from "$lib/components/common/RibbonButton.svelte";
  import DateField from "$lib/components/common/DateField.svelte";
  import TimeField from "$lib/components/common/TimeField.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { fmtDate, startOfDay, toInputTime, toTs } from "$lib/format";
  import type { Task } from "$lib/types";
  import ListPlus from "@lucide/svelte/icons/list-plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import Download from "@lucide/svelte/icons/download";
  import Mail from "@lucide/svelte/icons/mail";
  import Bell from "@lucide/svelte/icons/bell";
  import AlertCircle from "@lucide/svelte/icons/circle-alert";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import Sun from "@lucide/svelte/icons/sun";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Flag from "@lucide/svelte/icons/flag";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Layers from "@lucide/svelte/icons/layers";
  import { save as saveDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";

  let tasks = $state<Task[]>([]);
  let view = $state("active");
  let quick = $state("");
  let selected = $state<number | null>(null);
  let editOpen = $state(false);
  let edit = $state<Task>(blank());
  let reminderDate = $state<number | null>(null);
  let reminderTime = $state("09:00");

  function blank(): Task {
    return { id: 0, title: "", notes: "", start_date: null, due_date: null, reminder: null, reminder_done: false, priority: 1, status: "not_started", percent: 0, category: "", message_id: null, completed_at: null };
  }

  async function load() {
    tasks = await api.listTasks();
  }

  $effect(() => {
    void app.revision.tasks;
    void load();
  });

  $effect(() => {
    const r = app.newItemRequest;
    if (r && r.module === "tasks") {
      app.newItemRequest = null;
      untrack(() => openEdit({ ...blank(), ...(r.prefill as Partial<Task>) }));
    }
  });
  $effect(() => {
    const r = app.openItemRequest;
    if (r && r.module === "tasks") {
      app.openItemRequest = null;
      api.getTask(r.id).then((x) => x && openEdit(x));
    }
  });

  const today = $derived(toTs(startOfDay(new Date())));

  const VIEWS = [
    { key: "active", icon: ListTodo, label: "tasks.view_active" },
    { key: "today", icon: Sun, label: "tasks.view_today" },
    { key: "week", icon: CalendarDays, label: "tasks.view_week" },
    { key: "overdue", icon: TriangleAlert, label: "tasks.view_overdue" },
    { key: "flagged", icon: Flag, label: "tasks.view_flagged" },
    { key: "completed", icon: CircleCheck, label: "tasks.view_completed" },
    { key: "all", icon: Layers, label: "tasks.view_all" },
  ];

  function matchView(x: Task, v: string): boolean {
    const done = x.status === "completed";
    switch (v) {
      case "active": return !done;
      case "today": return !done && x.due_date != null && x.due_date < today + 86400;
      case "week": return !done && x.due_date != null && x.due_date < today + 7 * 86400;
      case "overdue": return !done && x.due_date != null && x.due_date < today;
      case "flagged": return !done && x.message_id != null;
      case "completed": return done;
      default: return true;
    }
  }

  const visible = $derived(tasks.filter((x) => matchView(x, view) && (!app.search || (x.title + " " + x.notes).toLocaleLowerCase().includes(app.search.toLocaleLowerCase()))));

  async function addQuick() {
    if (!quick.trim()) return;
    const due = view === "today" ? today + 17 * 3600 : view === "week" ? today + 7 * 86400 : null;
    await api.saveTask({ ...blank(), title: quick.trim(), due_date: due });
    quick = "";
    await load();
  }

  async function toggleDone(x: Task) {
    await api.saveTask({ ...x, status: x.status === "completed" ? "not_started" : "completed", percent: x.status === "completed" ? 0 : 100 });
    await load();
    app.bump("mail");
  }

  function openEdit(x: Task) {
    edit = { ...x };
    reminderDate = x.reminder;
    reminderTime = x.reminder ? toInputTime(x.reminder) : "09:00";
    editOpen = true;
  }

  async function save() {
    if (!edit.title.trim()) edit.title = t("tasks.untitled");
    let reminder: number | null = null;
    if (reminderDate) {
      const d = new Date(reminderDate * 1000);
      const [h, m] = reminderTime.split(":").map(Number);
      d.setHours(h, m, 0, 0);
      reminder = Math.floor(d.getTime() / 1000);
    }
    const reminderChanged = reminder !== edit.reminder;
    try {
      await api.saveTask({ ...edit, reminder, reminder_done: reminderChanged ? false : edit.reminder_done });
      editOpen = false;
      await load();
    } catch (e) {
      app.error(e);
    }
  }

  async function remove(ids: number[]) {
    if (!ids.length) return;
    if (await app.confirm(t("tasks.delete_confirm", ids.length), undefined, { ok: t("common.delete"), destructive: true })) {
      await api.deleteTasks(ids);
      selected = null;
      editOpen = false;
      await load();
    }
  }

  async function exportTasks() {
    const p = await saveDialog({ defaultPath: "gorevler.ics", filters: [{ name: "iCalendar", extensions: ["ics"] }] });
    if (p) toast.success(t("export.done", await api.exportTasks(p)));
  }

  function count(v: string) {
    return tasks.filter((x) => matchView(x, v)).length;
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  <Ribbon tabs={[{ key: "home", label: t("ribbon.home") }, { key: "help", label: t("ribbon.help") }]}>
    {#snippet file()}<FileMenu />{/snippet}
    {#snippet content(tab)}
      {#if tab === "help"}
        <div class="ribbon-group border-r-0"><RibbonButton icon={Info} label={t("settings.tab_about")} onclick={() => app.openSettings("about")} /></div>
      {:else}
    <div class="ribbon-group">
      <RibbonButton icon={ListPlus} label={t("tasks.new")} accent onclick={() => openEdit(blank())} />
    </div>
    <div class="ribbon-group">
      <RibbonButton icon={CheckCheck} label={t("tasks.mark_complete")} disabled={!selected} onclick={() => { const x = tasks.find((y) => y.id === selected); if (x) toggleDone(x); }} />
      <RibbonButton icon={Trash2} label={t("common.delete")} disabled={!selected} onclick={() => selected && remove([selected])} />
    </div>
    <div class="ribbon-group border-r-0">
      <RibbonButton icon={Download} label={t("common.export")} onclick={exportTasks} />
    </div>
      {/if}
    {/snippet}
  </Ribbon>

  <div class="pane mx-0.5 mb-0.5 flex min-h-0 flex-1 flex-col">
  <div class="flex min-h-0 flex-1">
    <nav class="bg-muted/20 w-52 shrink-0 space-y-0.5 border-r p-2">
      {#each VIEWS as v (v.key)}
        <button class="folder-item" data-selected={view === v.key} onclick={() => (view = v.key)}>
          <v.icon class="size-4 opacity-70" /><span class="flex-1">{t(v.label)}</span>
          {#if count(v.key) && v.key !== "all" && v.key !== "completed"}<span class="text-muted-foreground text-xs">{count(v.key)}</span>{/if}
        </button>
      {/each}
    </nav>
    <div class="flex min-w-0 flex-1 flex-col">
      <form class="flex gap-2 border-b p-3" onsubmit={(e) => { e.preventDefault(); addQuick(); }}>
        <Input placeholder={t("tasks.quick_add")} bind:value={quick} />
        <Button type="submit" disabled={!quick.trim()}>{t("common.add")}</Button>
      </form>
      <div class="text-muted-foreground grid grid-cols-[32px_24px_1fr_130px_110px_60px] gap-2 border-b px-3 py-1 text-xs font-medium">
        <span></span><span>!</span><span>{t("tasks.subject")}</span><span>{t("tasks.due")}</span><span>{t("tasks.status")}</span><span>%</span>
      </div>
      <div class="flex-1 overflow-y-auto">
        {#each visible as x (x.id)}
          {@const overdue = x.status !== "completed" && x.due_date != null && x.due_date < today}
          <div
            role="row"
            tabindex="0"
            class="grid cursor-default grid-cols-[32px_24px_1fr_130px_110px_60px] items-center gap-2 border-b px-3 py-2 text-sm {selected === x.id ? 'bg-primary/12' : 'hover:bg-accent/60'}"
            onclick={() => (selected = x.id)}
            ondblclick={() => openEdit(x)}
            onkeydown={(e) => { if (e.key === "Enter") openEdit(x); if (e.key === "Delete") remove([x.id]); }}
          >
            <Checkbox checked={x.status === "completed"} onCheckedChange={() => toggleDone(x)} onclick={(e) => e.stopPropagation()} />
            <span>{#if x.priority === 2}<AlertCircle class="size-4 text-red-600" />{:else if x.priority === 0}<ArrowDown class="size-4 text-blue-500" />{/if}</span>
            <span class="flex min-w-0 items-center gap-1.5">
              <span class="truncate {x.status === 'completed' ? 'text-muted-foreground line-through' : ''}">{x.title}</span>
              {#if x.message_id}<Mail class="text-muted-foreground size-3.5 shrink-0" />{/if}
              {#if x.reminder && !x.reminder_done}<Bell class="text-muted-foreground size-3.5 shrink-0" />{/if}
              {#if x.category}<span class="size-2.5 shrink-0 rounded-sm" style="background:{app.categoryColor(x.category)}"></span>{/if}
            </span>
            <span class="{overdue ? 'font-medium text-red-600' : ''}">{x.due_date ? fmtDate(x.due_date, { weekday: "short", day: "numeric", month: "short" }) : "—"}</span>
            <span class="text-xs">{t(`status.${x.status}`)}</span>
            <span class="text-xs">{x.percent}%</span>
          </div>
        {:else}
          <div class="text-muted-foreground p-10 text-center text-sm">{t("tasks.empty")}</div>
        {/each}
      </div>
    </div>
  </div>
  </div>
</div>

<Dialog.Root bind:open={editOpen}>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-xl">
    <Dialog.Header><Dialog.Title>{edit.id ? t("tasks.edit") : t("tasks.new")}</Dialog.Title></Dialog.Header>
    <div class="grid gap-3">
      <Input placeholder={t("tasks.subject")} bind:value={edit.title} class="font-medium" />
      <div class="grid grid-cols-[110px_1fr] items-center gap-2 text-sm">
        <Label>{t("tasks.start")}</Label><DateField bind:value={edit.start_date} clearable />
        <Label>{t("tasks.due")}</Label><DateField bind:value={edit.due_date} clearable />
        <Label>{t("tasks.status")}</Label>
        <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={edit.status} onchange={() => { if (edit.status === "completed") edit.percent = 100; }}>
          {#each ["not_started", "in_progress", "completed", "waiting", "deferred"] as s (s)}<option value={s}>{t(`status.${s}`)}</option>{/each}
        </select>
        <Label>{t("tasks.priority")}</Label>
        <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={edit.priority}>
          <option value={0}>{t("importance.low")}</option><option value={1}>{t("importance.normal")}</option><option value={2}>{t("importance.high")}</option>
        </select>
        <Label>{t("tasks.percent")}</Label>
        <div class="flex items-center gap-2"><input type="range" min="0" max="100" step="5" bind:value={edit.percent} class="flex-1" /><span class="w-10 text-right">{edit.percent}%</span></div>
        <Label>{t("tasks.reminder")}</Label>
        <div class="flex items-center gap-2"><DateField bind:value={reminderDate} clearable />{#if reminderDate}<TimeField bind:value={reminderTime} />{/if}</div>
        <Label>{t("mail.categorize")}</Label>
        <select class="border-input h-9 rounded-md border bg-transparent px-2" bind:value={edit.category}>
          <option value="">{t("calendar.no_category")}</option>
          {#each app.categories as c (c.name)}<option value={c.name}>{c.name}</option>{/each}
        </select>
      </div>
      <Textarea rows={6} placeholder={t("tasks.notes")} bind:value={edit.notes} />
      {#if edit.message_id}
        <Button variant="outline" size="sm" class="w-fit" onclick={() => { if (edit.message_id != null) app.openMessage(edit.message_id); }}><Mail class="size-4" />{t("tasks.open_mail")}</Button>
      {/if}
    </div>
    <Dialog.Footer class="sm:justify-between">
      <div>{#if edit.id}<Button variant="destructive" onclick={() => remove([edit.id])}>{t("common.delete")}</Button>{/if}</div>
      <div class="flex gap-2">
        <Button variant="outline" onclick={() => (editOpen = false)}>{t("common.cancel")}</Button>
        <Button onclick={save}>{t("common.save")}</Button>
      </div>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
