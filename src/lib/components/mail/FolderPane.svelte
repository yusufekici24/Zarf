<script lang="ts">
  // Klasör ağacı: hesaplar, yerel klasörler ve arama klasörleri.
  import * as ContextMenu from "$lib/components/ui/context-menu";
  import { ScrollArea } from "$lib/components/ui/scroll-area";
  import { app } from "$lib/stores/app.svelte";
  import { mail, SEARCH_FOLDERS } from "$lib/stores/mail.svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import type { Account, Folder } from "$lib/types";
  import Inbox from "@lucide/svelte/icons/inbox";
  import Send from "@lucide/svelte/icons/send";
  import FileText from "@lucide/svelte/icons/file-text";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import ShieldAlert from "@lucide/svelte/icons/shield-alert";
  import Archive from "@lucide/svelte/icons/archive";
  import Clock from "@lucide/svelte/icons/clock";
  import FolderIcon from "@lucide/svelte/icons/folder";
  import FolderSearch from "@lucide/svelte/icons/folder-search";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Monitor from "@lucide/svelte/icons/monitor";
  import AtSign from "@lucide/svelte/icons/at-sign";
  import Star from "@lucide/svelte/icons/star";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import { open as openDialog, save as saveDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";

  const ICONS: Record<string, typeof Inbox> = {
    inbox: Inbox, sent: Send, drafts: FileText, trash: Trash2, junk: ShieldAlert, archive: Archive, outbox: Clock,
  };

  let collapsed = $state<Record<string, boolean>>({});
  let menuTarget = $state<{ kind: "folder"; folder: Folder } | { kind: "account"; account: Account | null } | null>(null);

  function children(folders: Folder[], parent: number | null): Folder[] {
    const ids = new Set(folders.map((f) => f.id));
    return folders.filter((f) => (parent == null ? f.parent_id == null || !ids.has(f.parent_id) : f.parent_id === parent));
  }

  async function newFolder(accountId: number | null, parentId: number | null) {
    const name = await app.prompt(t("folder.new"), t("folder.name"));
    if (name) await api.createFolder(accountId, name, parentId);
    await app.loadFolders();
  }

  async function rename(f: Folder) {
    const name = await app.prompt(t("folder.rename"), t("folder.name"), f.name);
    if (name) await api.renameFolder(f.id, name);
    await app.loadFolders();
  }

  async function remove(f: Folder) {
    if (await app.confirm(t("folder.confirm_delete", f.display), t("folder.confirm_delete_text"), { ok: t("common.delete"), destructive: true })) {
      await api.deleteFolder(f.id);
      if (mail.folderId === f.id) mail.folderId = null;
      await app.loadFolders();
    }
  }

  async function empty(f: Folder) {
    if (await app.confirm(t("folder.confirm_empty", f.display), t("folder.confirm_empty_text"), { ok: t("folder.empty"), destructive: true })) {
      await api.emptyFolder(f.id);
    }
  }

  async function exportMbox(f: Folder) {
    const path = await saveDialog({ defaultPath: `${f.display}.mbox`, filters: [{ name: "mbox", extensions: ["mbox"] }] });
    if (path) toast.success(t("export.done", await api.exportMbox(f.id, path)));
  }

  async function importFiles(f: Folder) {
    const paths = await openDialog({ multiple: true, filters: [{ name: t("filter.mail_files"), extensions: ["eml", "msg", "mbox"] }] });
    if (paths) {
      const n = await api.importMailFiles(f.id, Array.isArray(paths) ? paths : [paths]);
      toast.success(t("import.messages_done", n));
    }
  }

  async function toggleFavorite(f: Folder) {
    await api.setFolderFavorite(f.id, !f.favorite);
    await app.loadFolders();
  }

  function accountOf(f: Folder): string {
    return app.accounts.find((a) => a.id === f.account_id)?.email ?? t("folder.local");
  }

  // Şeridin "Klasör" sekmesinden gelen istekler
  import { onMount } from "svelte";
  onMount(() => {
    const nf = (e: Event) => {
      const id = (e as CustomEvent<number | null>).detail;
      const f = id != null ? mail.folder(id) : undefined;
      void newFolder(f?.account_id ?? app.accounts[0]?.id ?? null, f?.id ?? null);
    };
    const ef = (e: Event) => {
      const f = mail.folder((e as CustomEvent<number>).detail);
      if (f) void empty(f);
    };
    window.addEventListener("mailnix:new-folder", nf);
    window.addEventListener("mailnix:empty-folder", ef);
    return () => {
      window.removeEventListener("mailnix:new-folder", nf);
      window.removeEventListener("mailnix:empty-folder", ef);
    };
  });

  async function runRules(f: Folder) {
    toast.success(t("rules.applied", await api.runRules(f.id)));
  }
</script>

{#snippet folderRow(f: Folder, folders: Folder[], depth: number, fav = false)}
  {@const Icon = ICONS[f.role ?? ""] ?? FolderIcon}
  {@const subs = fav ? [] : children(folders, f.id)}
  {@const key = `f${f.id}`}
  <div class="flex items-center" style="padding-left: {depth * 14}px">
    {#if subs.length}
      <button class="hover:bg-accent rounded p-0.5" onclick={() => (collapsed[key] = !collapsed[key])} aria-label="toggle">
        <ChevronRight class="size-3.5 transition-transform {collapsed[key] ? '' : 'rotate-90'}" />
      </button>
    {:else}
      <span class="w-4.5"></span>
    {/if}
    <button
      class="folder-item {mail.drag ? 'hover:ring-primary hover:ring-2' : ''}"
      data-selected={mail.folderId === f.id && !mail.searchFolder}
      data-folder-id={f.id}
      onclick={() => mail.selectFolder(f.id)}
      oncontextmenu={() => (menuTarget = { kind: "folder", folder: f })}
    >
      <Icon class="size-4 shrink-0 opacity-70" />
      <span class="flex-1 truncate {f.unread && f.role !== 'sent' ? 'font-semibold' : ''}">
        {f.display}{#if fav && app.multiAccount}<span class="text-muted-foreground ml-1 text-xs font-normal">· {accountOf(f)}</span>{/if}
      </span>
      {#if f.role === "drafts" || f.role === "outbox"}
        {#if f.total}<span class="text-primary text-xs">[{f.total}]</span>{/if}
      {:else if f.unread && f.role !== "sent" && f.role !== "trash"}
        <span class="text-primary text-xs font-semibold">{f.unread}</span>
      {/if}
    </button>
  </div>
  {#if !collapsed[key]}
    {#each subs as s (s.id)}{@render folderRow(s, folders, depth + 1)}{/each}
  {/if}
{/snippet}

<ContextMenu.Root>
  <ContextMenu.Trigger class="flex h-full flex-col">
    <ScrollArea class="h-full">
      <div class="space-y-3 p-2">
        {#if app.favorites.length}
          <div>
            <button
              class="hover:bg-accent flex w-full items-center gap-1.5 rounded px-1 py-1 text-left text-xs font-semibold tracking-wide uppercase opacity-80"
              onclick={() => (collapsed.fav = !collapsed.fav)}
            >
              <ChevronRight class="size-3.5 transition-transform {collapsed.fav ? '' : 'rotate-90'}" />
              <Star class="size-3.5" /><span class="normal-case">{t("folder.favorites")}</span>
            </button>
            {#if !collapsed.fav}
              {#each app.favorites as f (f.id)}{@render folderRow(f, [], 0, true)}{/each}
            {/if}
          </div>
        {/if}
        {#each app.folderGroups as g, gi (g.account?.id ?? "local")}
          {@const gkey = `a${g.account?.id ?? "local"}`}
          <div>
            <button
              class="hover:bg-accent flex w-full items-center gap-1.5 rounded px-1 py-1 text-left text-xs font-semibold tracking-wide uppercase opacity-80"
              onclick={() => (collapsed[gkey] = !collapsed[gkey])}
              oncontextmenu={() => (menuTarget = { kind: "account", account: g.account })}
            >
              <ChevronRight class="size-3.5 transition-transform {collapsed[gkey] ? '' : 'rotate-90'}" />
              {#if g.account}<AtSign class="size-3.5" />{:else}<Monitor class="size-3.5" />{/if}
              <span class="truncate normal-case">{g.account ? g.account.email : t("folder.local")}</span>
            </button>
            {#if g.account?.auth_state === "failed"}
              <button
                class="mx-1 mb-1 flex w-[calc(100%-0.5rem)] items-center gap-1.5 rounded-md bg-amber-100 px-2 py-1 text-left text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100"
                title={g.account.auth_error}
                onclick={() => (app.accountDialog = { open: true, account: g.account })}
              >
                <KeyRound class="size-3.5 shrink-0" /><span class="truncate">{t("account.paused_short")}</span>
              </button>
            {/if}
            {#if !collapsed[gkey]}
              {#each children(g.folders, null) as f (f.id)}{@render folderRow(f, g.folders, 0)}{/each}
              {#if gi === 0 && app.outbox && app.outbox.total > 0}{@render folderRow(app.outbox, [], 0)}{/if}
            {/if}
          </div>
        {/each}
        <div>
          <button class="hover:bg-accent flex w-full items-center gap-1.5 rounded px-1 py-1 text-left text-xs font-semibold tracking-wide uppercase opacity-80" onclick={() => (collapsed.search = !collapsed.search)}>
            <ChevronRight class="size-3.5 transition-transform {collapsed.search ? '' : 'rotate-90'}" />
            <FolderSearch class="size-3.5" /> <span class="normal-case">{t("folder.search_folders")}</span>
          </button>
          {#if !collapsed.search}
            {#each SEARCH_FOLDERS as s (s.key)}
              <div class="pl-4.5">
                <button class="folder-item" data-selected={mail.searchFolder === s.key} onclick={() => mail.selectSearchFolder(s.key)}>
                  <FolderSearch class="size-4 opacity-70" />
                  <span class="flex-1 truncate">{t(s.label)}</span>
                  {#if s.key === "unread" && app.unreadTotal}<span class="text-primary text-xs font-semibold">{app.unreadTotal}</span>{/if}
                </button>
              </div>
            {/each}
          {/if}
        </div>
      </div>
    </ScrollArea>
  </ContextMenu.Trigger>
  <ContextMenu.Content class="w-60">
    {#if menuTarget?.kind === "folder"}
      {@const f = menuTarget.folder}
      <ContextMenu.Item onclick={() => toggleFavorite(f)}>
        <Star class="size-4" />{f.favorite ? t("folder.remove_favorite") : t("folder.add_favorite")}
      </ContextMenu.Item>
      <ContextMenu.Separator />
      <ContextMenu.Item onclick={() => newFolder(f.account_id, f.id)}>{t("folder.new_sub")}</ContextMenu.Item>
      <ContextMenu.Item disabled={!!f.role} onclick={() => rename(f)}>{t("folder.rename")}</ContextMenu.Item>
      <ContextMenu.Item disabled={!!f.role} onclick={() => remove(f)}>{t("folder.delete")}</ContextMenu.Item>
      <ContextMenu.Separator />
      <ContextMenu.Item onclick={() => api.markFolderRead(f.id)}>{t("folder.mark_all_read")}</ContextMenu.Item>
      <ContextMenu.Item onclick={() => empty(f)}>{t("folder.empty")}</ContextMenu.Item>
      <ContextMenu.Item onclick={() => runRules(f)}>{t("rules.run_now")}</ContextMenu.Item>
      <ContextMenu.Separator />
      {#if f.account_id}<ContextMenu.Item onclick={() => api.syncFolder(f.id)}>{t("folder.sync")}</ContextMenu.Item>{/if}
      <ContextMenu.Item onclick={() => importFiles(f)}>{t("folder.import")}</ContextMenu.Item>
      <ContextMenu.Item onclick={() => exportMbox(f)}>{t("folder.export_mbox")}</ContextMenu.Item>
    {:else if menuTarget?.kind === "account"}
      {@const a = menuTarget.account}
      <ContextMenu.Item onclick={() => newFolder(a?.id ?? null, null)}>{t("folder.new")}</ContextMenu.Item>
      {#if a}
        <ContextMenu.Item onclick={() => api.syncAccount(a.id)}>{t("account.sync")}</ContextMenu.Item>
        <ContextMenu.Item onclick={() => (app.accountDialog = { open: true, account: a })}>{t("account.settings")}</ContextMenu.Item>
      {/if}
    {:else}
      <ContextMenu.Item onclick={() => newFolder(null, null)}>{t("folder.new")}</ContextMenu.Item>
    {/if}
  </ContextMenu.Content>
</ContextMenu.Root>
