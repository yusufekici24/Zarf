<script lang="ts">
  import Ribbon from "$lib/components/ribbon/Ribbon.svelte";
  import FileMenu from "$lib/components/ribbon/FileMenu.svelte";
  import Info from "@lucide/svelte/icons/info";
  import Avatar from "$lib/components/common/Avatar.svelte";
  // Kişiler modülü: liste + kişi kartı + kişi grupları.
  import * as Dialog from "$lib/components/ui/dialog";
  import Splitter from "$lib/components/common/Splitter.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Textarea } from "$lib/components/ui/textarea";
  import RibbonButton from "$lib/components/common/RibbonButton.svelte";
  import AddressInput from "$lib/components/common/AddressInput.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { avatarColor, initials, splitNameAddr } from "$lib/format";
  import type { Contact, ContactGroup } from "$lib/types";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import UsersRound from "@lucide/svelte/icons/users-round";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Upload from "@lucide/svelte/icons/upload";
  import Download from "@lucide/svelte/icons/download";
  import Mail from "@lucide/svelte/icons/mail";
  import CalendarPlus from "@lucide/svelte/icons/calendar-plus";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Building2 from "@lucide/svelte/icons/building-2";
  import Star from "@lucide/svelte/icons/star";
  import Phone from "@lucide/svelte/icons/phone";
  import Building from "@lucide/svelte/icons/building-2";
  import MapPin from "@lucide/svelte/icons/map-pin";
  import Globe from "@lucide/svelte/icons/globe";
  import Cake from "@lucide/svelte/icons/cake";
  import Users from "@lucide/svelte/icons/users";
  import { open as openDialog, save as saveDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";
  import { onMount, untrack } from "svelte";

  let sideW = $state(Number(app.setting("pane_contacts_w", "340")) || 340);
  let contacts = $state<Contact[]>([]);
  let groups = $state<ContactGroup[]>([]);
  let selectedId = $state<number | null>(null);
  let selectedGroup = $state<number | null>(null);
  let checked = $state<Set<number>>(new Set());
  let editOpen = $state(false);
  let edit = $state<Contact>(blank());
  let groupOpen = $state(false);
  let group = $state<ContactGroup>({ id: 0, name: "", members: "" });

  function blank(): Contact {
    return { id: 0, first_name: "", last_name: "", display_name: "", email: "", email2: "", phone_mobile: "", phone_work: "", phone_home: "", company: "", job_title: "", department: "", address: "", website: "", birthday: "", notes: "", categories: "", is_favorite: false };
  }

  let loadRequest = 0;
  async function load() {
    const request = ++loadRequest;
    try {
      const [nextContacts, nextGroups] = await Promise.all([api.listContacts(app.search), api.listGroups()]);
      if (request !== loadRequest) return;
      contacts = nextContacts;
      groups = nextGroups;
    } catch (error) { if (request === loadRequest) app.error(error); }
  }

  $effect(() => {
    void app.revision.contacts;
    void app.search;
    const timer = setTimeout(() => void load(), 180);
    return () => { clearTimeout(timer); ++loadRequest; };
  });

  $effect(() => {
    const r = app.newItemRequest;
    if (r && r.module === "contacts") {
      app.newItemRequest = null;
      untrack(() => openEdit({ ...blank(), ...(r.prefill as Partial<Contact>) }));
    }
  });
  $effect(() => {
    const r = app.openItemRequest;
    if (r && r.module === "contacts") {
      app.openItemRequest = null;
      selectedId = r.id;
      selectedGroup = null;
    }
  });

  onMount(() => {
    const h = (e: Event) => {
      const { name, addr } = splitNameAddr(String((e as CustomEvent).detail ?? ""));
      const parts = name.split(/\s+/).filter(Boolean);
      app.module = "contacts";
      openEdit({ ...blank(), first_name: parts.length > 1 ? parts.slice(0, -1).join(" ") : parts[0] ?? "", last_name: parts.length > 1 ? parts[parts.length - 1] : "", display_name: name, email: addr });
    };
    window.addEventListener("lynora:add-contact", h);
    return () => window.removeEventListener("lynora:add-contact", h);
  });

  const selected = $derived(contacts.find((c) => c.id === selectedId) ?? null);
  const selGroup = $derived(groups.find((g) => g.id === selectedGroup) ?? null);

  // Baş harfe göre gruplama
  let contactPage = $state(0);
  const pageSize = 100;
  const contactPages = $derived(Math.max(1, Math.ceil(contacts.length / pageSize)));
  $effect(() => { void contacts; contactPage = 0; });
  const letters = $derived.by(() => {
    const map = new Map<string, Contact[]>();
    for (const c of contacts.slice(contactPage * pageSize, (contactPage + 1) * pageSize)) {
      const key = c.is_favorite ? "★" : (c.display_name[0] ?? "#").toLocaleUpperCase();
      const bucket = map.get(key);
      if (bucket) bucket.push(c);
      else map.set(key, [c]);
    }
    return [...map.entries()];
  });

  const canEdit = (c: Contact) => !c.account_id || !!app.accounts.find(a => a.id === c.account_id)?.carddav_url;
  const source = (c: Contact) => c.account_id ? app.accounts.find(a => a.id === c.account_id)?.email || t("contacts.from_exchange") : t("contacts.local");
  function copyContact(c: Contact) { openEdit({ ...c, id: 0, account_id: null, remote_id: null }); }
  function openEdit(c: Contact) {
    if (!canEdit(c)) { toast.info(t("contacts.readonly")); return; }
    edit = { ...c };
    editOpen = true;
  }

  async function saveContact() {
    try {
      if (!edit.display_name.trim() && !edit.first_name.trim() && !edit.email.trim()) { toast.error(t("contacts.required")); return; }
      if (!edit.id && edit.email.trim()) {
        const existing = await api.contactByEmail(edit.email.trim());
        if (existing && !(await app.confirm(t("contacts.duplicate", edit.email)))) return;
      }
      const id = await api.saveContact(edit);
      editOpen = false;
      selectedId = id;
      selectedGroup = null;
      await load();
    } catch (e) {
      app.error(e);
    }
  }

  async function removeSelected() {
    const ids = (checked.size ? [...checked] : selectedId ? [selectedId] : []).filter(id => contacts.some(c => c.id === id && canEdit(c)));
    if (selGroup && !ids.length) {
      if (await app.confirm(t("contacts.delete_group", selGroup.name), undefined, { ok: t("common.delete"), destructive: true })) {
        await api.deleteGroup(selGroup.id);
        selectedGroup = null;
        await load();
      }
      return;
    }
    if (!ids.length) return;
    if (await app.confirm(t("contacts.delete_confirm", ids.length), undefined, { ok: t("common.delete"), destructive: true })) {
      await api.deleteContacts(ids);
      checked = new Set();
      selectedId = null;
      await load();
    }
  }

  async function toggleFav(c: Contact) {
    if (!canEdit(c)) return;
    await api.saveContact({ ...c, is_favorite: !c.is_favorite });
    await load();
  }

  async function importContacts() {
    const p = await openDialog({ filters: [{ name: t("filter.contacts"), extensions: ["vcf", "vcard", "csv", "msg"] }] });
    if (p && typeof p === "string") {
      toast.success(t("import.contacts_done", await api.importContacts(p)));
      await load();
    }
  }

  async function exportContacts() {
    const p = await saveDialog({ defaultPath: "kisiler.vcf", filters: [{ name: "vCard", extensions: ["vcf"] }, { name: "CSV", extensions: ["csv"] }] });
    if (p) toast.success(t("export.done", await api.exportContacts(p, checked.size ? [...checked] : null)));
  }

  function mailTo(c: Contact | ContactGroup) {
    const to = "members" in c ? c.members : `${c.display_name} <${c.email}>`;
    app.compose({ mode: "new", data: { to } });
  }

  function meeting(c: Contact | ContactGroup) {
    const attendees = "members" in c ? c.members.split(/[,;]/).map((x) => splitNameAddr(x).addr).filter(Boolean).join(",") : c.email;
    app.newItem("calendar", { attendees, title: "" });
  }

  async function saveGroup() {
    if (!group.name.trim()) return;
    selectedGroup = await api.saveGroup(group);
    selectedId = null;
    groupOpen = false;
    await load();
  }

  const fields: [keyof Contact, string][] = [
    ["first_name", "contacts.first_name"], ["last_name", "contacts.last_name"], ["display_name", "contacts.display_name"],
    ["email", "contacts.email"], ["email2", "contacts.email2"], ["phone_mobile", "contacts.mobile"], ["phone_work", "contacts.work_phone"],
    ["phone_home", "contacts.home_phone"], ["company", "contacts.company"], ["job_title", "contacts.job_title"], ["department", "contacts.department"],
    ["website", "contacts.website"], ["birthday", "contacts.birthday"], ["categories", "contacts.categories"],
  ];
</script>

<div class="flex h-full min-h-0 flex-col">
  <Ribbon tabs={[{ key: "home", label: t("ribbon.home") }, { key: "help", label: t("ribbon.help") }]}>
    {#snippet file()}<FileMenu />{/snippet}
    {#snippet content(tab)}
      {#if tab === "help"}
        <div class="ribbon-group border-r-0"><RibbonButton icon={Info} label={t("settings.tab_about")} onclick={() => app.openSettings("about")} /></div>
      {:else}
    <div class="ribbon-group">
      <RibbonButton icon={UserPlus} label={t("contacts.new")} accent onclick={() => openEdit(blank())} />
      <RibbonButton icon={UsersRound} label={t("contacts.new_group")} onclick={() => { group = { id: 0, name: "", members: "" }; groupOpen = true; }} />
    </div>
    <div class="ribbon-group">
      <RibbonButton icon={Trash2} label={t("common.delete")} disabled={!selectedId && !checked.size && !selGroup} onclick={removeSelected} />
    </div>
    <div class="ribbon-group border-r-0">
      <RibbonButton icon={Upload} label={t("common.import")} onclick={importContacts} />
      <RibbonButton icon={Download} label={t("common.export")} onclick={exportContacts} />
    </div>
      {/if}
    {/snippet}
  </Ribbon>

  <div class="pane mx-0.5 mb-0.5 flex min-h-0 flex-1 flex-col">
  <div class="flex min-h-0 flex-1">
    <div class="min-w-0 shrink-0 overflow-hidden " style="width:{sideW}px">
      <div class="h-full overflow-y-auto">
        {#if groups.length}
          <div class="bg-muted/50 px-3 py-1 text-xs font-semibold">{t("contacts.groups")}</div>
          {#each groups as g (g.id)}
            <button class="flex w-full items-center gap-2 border-b px-3 py-2 text-left text-sm {selectedGroup === g.id ? 'bg-primary/12' : 'hover:bg-accent'}" onclick={() => { selectedGroup = g.id; selectedId = null; }}>
              <div class="bg-muted flex size-8 items-center justify-center rounded-full"><Users class="size-4" /></div>
              <span class="flex-1 truncate font-medium">{g.name}</span>
              <span class="text-muted-foreground text-xs">{g.members.split(/[,;]/).filter((x) => x.trim()).length}</span>
            </button>
          {/each}
        {/if}
        <div class="flex items-center justify-between gap-2 p-2">
          <Button size="sm" variant="outline" disabled={contactPage === 0} onclick={() => contactPage--}>‹</Button>
          <span class="text-xs">{contactPage + 1} / {contactPages} · {contacts.length}</span>
          <Button size="sm" variant="outline" disabled={contactPage + 1 >= contactPages} onclick={() => contactPage++}>›</Button>
        </div>
        {#each letters as [letter, list] (letter)}
          <div class="bg-muted/50 sticky top-0 px-3 py-1 text-xs font-semibold">{letter}</div>
          {#each list as c (c.id)}
            <div class="flex items-center gap-2 border-b px-3 py-2 text-sm {selectedId === c.id ? 'bg-primary/12' : 'hover:bg-accent'}">
              <input type="checkbox" disabled={!canEdit(c)} checked={checked.has(c.id)} onchange={(e) => { const s = new Set(checked); (e.target as HTMLInputElement).checked ? s.add(c.id) : s.delete(c.id); checked = s; }} />
              <button class="flex min-w-0 flex-1 items-center gap-2 text-left" onclick={() => { selectedId = c.id; selectedGroup = null; }} ondblclick={() => openEdit(c)}>
                <Avatar name={c.display_name} email={c.email} class="size-8 text-xs" />
                <div class="min-w-0">
                  <div class="truncate font-medium">{c.display_name}</div>
                  <div class="text-muted-foreground truncate text-xs">{c.email || c.phone_mobile || c.company}</div>
                </div>
              </button>
              {#if c.is_favorite}<Star class="size-3.5 fill-amber-400 text-amber-400" />{/if}
            </div>
          {/each}
        {:else}
          <div class="text-muted-foreground p-8 text-center text-sm">{app.search ? t("common.no_results") : t("contacts.empty")}</div>
        {/each}
      </div>
    </div>
    <Splitter bind:size={sideW} min={220} max={800} onend={(v) => app.setSetting("pane_contacts_w", String(v))} />
    <div class="min-w-0 flex-1">
      <div class="h-full overflow-y-auto p-6">
        {#if selected}
          {@const c = selected}
          <div class="flex items-center gap-4">
            <Avatar name={c.display_name} email={c.email} class="size-20 text-2xl" />
            <div class="selectable flex-1">
              <h2 class="text-2xl font-semibold">{c.display_name}</h2><p class="text-muted-foreground text-xs">{source(c)}</p>
              {#if c.job_title || c.company}<div class="text-muted-foreground">{[c.job_title, c.department, c.company].filter(Boolean).join(" · ")}</div>{/if}
            </div>
          </div>
          <div class="mt-4 flex flex-wrap gap-2">
            {#if c.email}<Button size="sm" onclick={() => mailTo(c)}><Mail class="size-4" />{t("contacts.send_mail")}</Button>{/if}
            <Button size="sm" variant="outline" onclick={() => meeting(c)}><CalendarPlus class="size-4" />{t("contacts.schedule")}</Button>
            {#if !canEdit(c)}
              <Button size="sm" variant="outline" onclick={() => copyContact(c)}>{t("contacts.copy")}</Button>
              <span class="text-muted-foreground flex items-center gap-1 text-xs"><Building2 class="size-3.5" />{t("contacts.from_exchange")}</span>
            {:else}
              <Button size="sm" variant="outline" onclick={() => openEdit(c)}><Pencil class="size-4" />{t("common.edit")}</Button>
            {/if}
            <Button size="sm" variant="outline" disabled={!canEdit(c)} onclick={() => toggleFav(c)}><Star class="size-4 {c.is_favorite ? 'fill-amber-400 text-amber-400' : ''}" />{c.is_favorite ? t("contacts.unfavorite") : t("contacts.favorite")}</Button>
          </div>
          <div class="selectable mt-6 grid max-w-2xl gap-3 text-sm">
            {#each ([[Mail, c.email], [Mail, c.email2], [Phone, c.phone_mobile && `${c.phone_mobile} (${t("contacts.mobile")})`], [Phone, c.phone_work && `${c.phone_work} (${t("contacts.work_phone")})`], [Phone, c.phone_home && `${c.phone_home} (${t("contacts.home_phone")})`], [Building, c.company], [MapPin, c.address], [Globe, c.website], [Cake, c.birthday]] as [typeof Mail, string][]) as [Icon, val], i (i)}
              {#if val}
                <div class="flex items-start gap-3">
                  <Icon class="text-muted-foreground mt-0.5 size-4" />
                  {#if Icon === Mail}<button class="text-primary hover:underline" onclick={() => app.compose({ mode: "new", data: { to: `${c.display_name} <${val}>` } })}>{val}</button>
                  {:else if Icon === Globe}<button class="text-primary hover:underline" onclick={() => void api.openUrl(String(val).startsWith("http") ? String(val) : "https://" + val)}>{val}</button>
                  {:else}<span>{val}</span>{/if}
                </div>
              {/if}
            {/each}
            {#if c.categories}<div class="flex flex-wrap gap-1">{#each c.categories.split(",").filter(Boolean) as cat (cat)}<span class="rounded px-2 py-0.5 text-xs text-white" style="background:{app.categoryColor(cat.trim())}">{cat}</span>{/each}</div>{/if}
            {#if c.notes}<div class="bg-muted/40 rounded-md p-3 whitespace-pre-wrap">{c.notes}</div>{/if}
          </div>
        {:else if selGroup}
          {@const g = selGroup}
          <div class="flex items-center gap-4">
            <div class="bg-muted flex size-20 items-center justify-center rounded-full"><Users class="size-10" /></div>
            <h2 class="flex-1 text-2xl font-semibold">{g.name}</h2>
          </div>
          <div class="mt-4 flex gap-2">
            <Button size="sm" onclick={() => mailTo(g)}><Mail class="size-4" />{t("contacts.send_mail")}</Button>
            <Button size="sm" variant="outline" onclick={() => meeting(g)}><CalendarPlus class="size-4" />{t("contacts.schedule")}</Button>
            <Button size="sm" variant="outline" onclick={() => { group = { ...g }; groupOpen = true; }}><Pencil class="size-4" />{t("common.edit")}</Button>
          </div>
          <div class="selectable mt-6 space-y-1 text-sm">
            {#each g.members.split(/[,;]/).map((x) => x.trim()).filter(Boolean) as m (m)}<div class="rounded border px-3 py-1.5">{m}</div>{/each}
          </div>
        {:else}
          <div class="text-muted-foreground flex h-full items-center justify-center text-sm">{t("contacts.select")}</div>
        {/if}
      </div>
    </div>
  </div>
  </div>
</div>

<Dialog.Root bind:open={editOpen}>
  <Dialog.Content class="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
    <Dialog.Header><Dialog.Title>{edit.id ? t("contacts.edit") : t("contacts.new")}</Dialog.Title></Dialog.Header>
    <div class="grid grid-cols-2 gap-3">
      {#each fields as [key, label] (key)}
        <div class="grid gap-1">
          <Label class="text-xs">{t(label)}</Label>
          <Input bind:value={edit[key] as string} type={key === "birthday" ? "date" : key.startsWith("email") ? "email" : "text"} />
        </div>
      {/each}
      <div class="col-span-2 grid gap-1"><Label class="text-xs">{t("contacts.address")}</Label><Textarea rows={2} bind:value={edit.address} /></div>
      <div class="col-span-2 grid gap-1"><Label class="text-xs">{t("contacts.notes")}</Label><Textarea rows={4} bind:value={edit.notes} /></div>
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (editOpen = false)}>{t("common.cancel")}</Button>
      <Button onclick={saveContact}>{t("common.save")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={groupOpen}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header><Dialog.Title>{group.id ? t("contacts.edit_group") : t("contacts.new_group")}</Dialog.Title><Dialog.Description>{t("contacts.group_help")}</Dialog.Description></Dialog.Header>
    <div class="grid gap-3">
      <div class="grid gap-1"><Label>{t("contacts.group_name")}</Label><Input bind:value={group.name} /></div>
      <div class="grid gap-1"><Label>{t("contacts.members")}</Label><div class="border-input rounded-md border px-2"><AddressInput bind:value={group.members} /></div></div>
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (groupOpen = false)}>{t("common.cancel")}</Button>
      <Button data-default-button onclick={saveGroup}>{t("common.save")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
