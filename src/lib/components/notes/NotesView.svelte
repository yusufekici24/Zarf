<script lang="ts">
  import Ribbon from "$lib/components/ribbon/Ribbon.svelte";
  import FileMenu from "$lib/components/ribbon/FileMenu.svelte";
  import Info from "@lucide/svelte/icons/info";
  // Notlar modülü: renkli yapışkan notlar.
  import RibbonButton from "$lib/components/common/RibbonButton.svelte";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { fmtDateTime } from "$lib/format";
  import type { Note } from "$lib/types";
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Palette from "@lucide/svelte/icons/palette";
  import Mail from "@lucide/svelte/icons/mail";
  import ListTodo from "@lucide/svelte/icons/list-todo";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";

  const COLORS: Record<string, string> = {
    yellow: "#fff3a8", green: "#c9f0c1", blue: "#cfe6ff", pink: "#ffd3e3", purple: "#e3d6ff", white: "#ffffff", gray: "#e4e4e4",
  };

  let notes = $state<Note[]>([]);
  let activeId = $state<number | null>(null);
  const timers = new Map<number, ReturnType<typeof setTimeout>>();

  async function load() {
    notes = await api.listNotes();
  }

  $effect(() => {
    void app.revision.notes;
    void load();
  });

  $effect(() => {
    const r = app.newItemRequest;
    if (r && r.module === "notes") {
      app.newItemRequest = null;
      void add(String((r.prefill as { body?: string } | undefined)?.body ?? ""));
    }
  });

  const visible = $derived(notes.filter((n) => !app.search || n.body.toLocaleLowerCase().includes(app.search.toLocaleLowerCase())));

  async function add(body = "") {
    const id = await api.saveNote({ id: 0, body, color: app.setting("note_color", "yellow"), category: "", modified: 0 });
    await load();
    activeId = id;
    setTimeout(() => (document.getElementById(`note-${id}`) as HTMLTextAreaElement | null)?.focus(), 50);
  }

  function edit(n: Note, body: string) {
    n.body = body;
    clearTimeout(timers.get(n.id));
    timers.set(n.id, setTimeout(() => api.saveNote(n), 500));
  }

  async function setColor(n: Note, color: string) {
    await api.saveNote({ ...n, color });
    await load();
  }

  async function remove(n: Note) {
    if (n.body.trim() && !(await app.confirm(t("notes.delete_confirm"), undefined, { ok: t("common.delete"), destructive: true }))) return;
    await api.deleteNote(n.id);
    await load();
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
      <RibbonButton icon={StickyNote} label={t("notes.new")} accent onclick={() => add()} />
    </div>
    <div class="ribbon-group border-r-0">
      <RibbonButton icon={Trash2} label={t("common.delete")} disabled={!activeId} onclick={() => { const n = notes.find((x) => x.id === activeId); if (n) remove(n); }} />
    </div>
      {/if}
    {/snippet}
  </Ribbon>

  <div class="pane mx-0.5 mb-0.5 flex min-h-0 flex-1 flex-col">
  <div class="flex-1 overflow-y-auto p-4">
    <div class="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
      {#each visible as n (n.id)}
        <div class="flex h-56 flex-col rounded-lg shadow-md ring-1 ring-black/5 {activeId === n.id ? 'ring-primary ring-2' : ''}" style="background:{COLORS[n.color] ?? COLORS.yellow}">
          <div class="flex items-center gap-1 px-2 pt-1 text-[11px] text-black/50">
            <span class="flex-1">{fmtDateTime(n.modified)}</span>
            <DropdownMenu.Root>
              <DropdownMenu.Trigger class="rounded p-0.5 hover:bg-black/10"><Ellipsis class="size-4" /></DropdownMenu.Trigger>
              <DropdownMenu.Content>
                <DropdownMenu.Sub>
                  <DropdownMenu.SubTrigger><Palette class="size-4" />{t("notes.color")}</DropdownMenu.SubTrigger>
                  <DropdownMenu.SubContent>
                    {#each Object.entries(COLORS) as [k, c] (k)}
                      <DropdownMenu.Item onclick={() => setColor(n, k)}><span class="size-4 rounded border" style="background:{c}"></span>{t(`notes.color_${k}`)}</DropdownMenu.Item>
                    {/each}
                  </DropdownMenu.SubContent>
                </DropdownMenu.Sub>
                <DropdownMenu.Item onclick={() => app.compose({ mode: "new", data: { subject: n.body.split("\n")[0].slice(0, 80), html: n.body.replace(/\n/g, "<br>") } })}><Mail class="size-4" />{t("notes.send_mail")}</DropdownMenu.Item>
                <DropdownMenu.Item onclick={() => app.newItem("tasks", { title: n.body.split("\n")[0].slice(0, 120), notes: n.body })}><ListTodo class="size-4" />{t("notes.to_task")}</DropdownMenu.Item>
                <DropdownMenu.Separator />
                <DropdownMenu.Item class="text-destructive" onclick={() => remove(n)}><Trash2 class="size-4" />{t("common.delete")}</DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </div>
          <textarea
            id="note-{n.id}"
            class="flex-1 resize-none bg-transparent px-3 pb-3 text-sm text-black outline-none"
            value={n.body}
            placeholder={t("notes.placeholder")}
            onfocus={() => (activeId = n.id)}
            oninput={(e) => edit(n, (e.target as HTMLTextAreaElement).value)}
          ></textarea>
        </div>
      {:else}
        <div class="text-muted-foreground col-span-full p-10 text-center text-sm">{t("notes.empty")}</div>
      {/each}
    </div>
  </div>
  </div>
</div>
