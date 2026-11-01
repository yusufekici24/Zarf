<script lang="ts">
  // Kuralları Şimdi Çalıştır: kurallar seçilen klasördeki mevcut iletilere uygulanır. Önizleme
  // hiçbir şeyi değiştirmez; çalıştırma ilerleme gösterir ve iptal edilebilir.
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Progress } from "$lib/components/ui/progress";
  import { Switch } from "$lib/components/ui/switch";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { displayName, fmtListDate } from "$lib/format";
  import type { LocalSpec, Rule, RuleReport, RuleRun } from "$lib/types";
  import { listen } from "@tauri-apps/api/event";
  import Loader from "@lucide/svelte/icons/loader-circle";

  let open = $state(true);
  let rules = $state<Rule[]>([]);
  let selected = $state<number[]>([]);
  let subfolders = $state(false);
  let read = $state<RuleRun["read"]>("all");
  let busy = $state(false);
  let progress = $state({ done: 0, total: 0, matched: 0 });
  let report = $state<RuleReport | null>(null);
  let previewed = $state(false);

  const folderName = $derived(app.folderGroups.flatMap((g) => g.folders).find((f) => f.id === app.runRulesFolder)?.name ?? "");

  $effect(() => {
    void api.listRules().then((all) => {
      rules = all.filter((r) => !r.remote_id && r.enabled);
      selected = rules.map((r) => r.id);
    });
    const off = listen<typeof progress>("rules://progress", (e) => (progress = e.payload));
    return () => void off.then((f) => f());
  });

  $effect(() => {
    if (!open) app.runRulesFolder = null;
  });

  function toggle(id: number, on: boolean) {
    selected = on ? [...selected, id] : selected.filter((x) => x !== id);
  }

  /** Seçili kurallardan biri iletiyi siliyor mu */
  function deletes(r: Rule): boolean {
    const kinds = ["delete", "permanent_delete"];
    if (!r.spec) return kinds.includes(r.action);
    try {
      return (JSON.parse(r.spec) as LocalSpec).actions.some((a) => kinds.includes(a.kind));
    } catch {
      return false;
    }
  }

  function actionLabel([action, arg]: [string, string]): string {
    const key = action === "category" ? "categories" : action === "important" ? "importance" : action;
    const name = t(`rw.a_${key}`);
    return arg ? `${name}: ${arg}` : name;
  }

  async function start(apply: boolean) {
    if (apply && rules.some((r) => selected.includes(r.id) && deletes(r))) {
      if (!(await app.confirm(t("rules.run_confirm_title"), t("rules.run_confirm_text"), { ok: t("rules.run_start"), destructive: true }))) return;
    }
    const run: RuleRun = { folder_id: app.runRulesFolder ?? 0, subfolders, read, rule_ids: $state.snapshot(selected) };
    busy = true;
    report = null;
    progress = { done: 0, total: 0, matched: 0 };
    try {
      report = await (apply ? api.runRules(run) : api.previewRules(run));
      previewed = !apply;
      if (apply) app.bump("mail");
    } catch (e) {
      app.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="flex max-h-[85vh] flex-col overflow-hidden sm:max-w-2xl">
    <Dialog.Header><Dialog.Title>{t("rules.run_title")}</Dialog.Title></Dialog.Header>
    <div class="min-h-0 shrink space-y-3 overflow-y-auto pr-1 text-sm">
      <div class="grid grid-cols-[140px_1fr] items-center gap-2">
        <span>{t("rules.run_folder")}</span>
        <span class="truncate font-medium">{folderName}</span>
        <span>{t("rules.run_which")}</span>
        <select class="border-input h-8 rounded-md border bg-transparent px-2 text-sm" bind:value={read} disabled={busy}>
          <option value="all">{t("rules.run_all")}</option>
          <option value="read">{t("rules.run_read")}</option>
          <option value="unread">{t("rules.run_unread")}</option>
        </select>
      </div>
      <label class="flex items-center gap-2">
        <Switch bind:checked={subfolders} disabled={busy} />
        {t("rules.run_subfolders")}
      </label>
      <div>
        <div class="mb-1 font-medium">{t("rules.run_rules")}</div>
        <div class="max-h-40 overflow-y-auto rounded-md border p-2">
          {#each rules as r (r.id)}
            <label class="hover:bg-accent/50 flex cursor-pointer items-center gap-2 rounded px-1 py-0.5">
              <input type="checkbox" class="accent-primary size-4" checked={selected.includes(r.id)} disabled={busy} onchange={(e) => toggle(r.id, (e.target as HTMLInputElement).checked)} />
              <span class="min-w-0 truncate">{r.name}</span>
            </label>
          {:else}
            <div class="text-muted-foreground p-3 text-center text-xs">{t("rules.run_none")}</div>
          {/each}
        </div>
      </div>

      {#if busy}
        <div class="space-y-1">
          <Progress value={progress.total ? (progress.done / progress.total) * 100 : 0} />
          <div class="text-muted-foreground text-xs">{t("rules.run_progress", progress.done, progress.total, progress.matched)}</div>
        </div>
      {:else if report}
        <div class="rounded-md bg-blue-50 px-3 py-1.5 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">
          {#if report.cancelled}{t("rules.run_cancelled")}{/if}
          {previewed ? t("rules.run_preview_result", report.scanned, report.matched) : t("rules.run_result", report.scanned, report.matched, report.applied)}
        </div>
        {#if report.hits.length}
          <div class="max-h-56 divide-y overflow-y-auto rounded-md border">
            {#each report.hits as h (h.id)}
              <div class="px-2 py-1">
                <div class="flex items-baseline gap-2">
                  <span class="min-w-0 flex-1 truncate font-medium">{h.subject || t("mail.no_subject")}</span>
                  <span class="text-muted-foreground shrink-0 text-xs">{fmtListDate(h.date)}</span>
                </div>
                <div class="text-muted-foreground truncate text-xs">{displayName(h.sender)} · {h.actions.map(actionLabel).join(", ")}</div>
              </div>
            {/each}
          </div>
          {#if report.matched > report.hits.length}
            <div class="text-muted-foreground text-xs">{t("rules.run_more", report.matched - report.hits.length)}</div>
          {/if}
        {/if}
      {/if}
    </div>
    <Dialog.Footer>
      {#if busy}
        <Button variant="outline" onclick={() => void api.cancelRuleRun()}>{t("common.cancel")}</Button>
      {:else}
        <Button variant="outline" onclick={() => (open = false)}>{t("common.close")}</Button>
        <Button variant="outline" disabled={!selected.length} onclick={() => start(false)}>{t("rules.preview")}</Button>
      {/if}
      <Button disabled={busy || !selected.length} onclick={() => start(true)}>
        {#if busy}<Loader class="size-4 animate-spin" />{/if}{t("rules.run_start")}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
