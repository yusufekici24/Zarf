<script lang="ts">
  // İleti şablonları: düzenlenecek şablonu seç (Sil / Yeni / Kaydet / Yeniden Adlandır), konu ve
  // biçimli gövde. Şablonlar yalnızca bu bilgisayarda saklanır, yeni ileti şeridinden eklenir.
  import RichEditor from "$lib/components/common/RichEditor.svelte";
  import { onDestroy } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { loadTemplates, saveTemplates, type Template } from "$lib/templates";

  let list = $state<Template[]>(loadTemplates());
  let sel = $state<string>(list[0]?.name ?? "");
  let subject = $state(list[0]?.subject ?? "");
  let html = $state(list[0]?.html ?? "");
  let dirty = $state(false);
  let editor: RichEditor | undefined = $state();

  const btn = "h-[26px] rounded-[3px] border border-[#8a8a8a] bg-[#fdfdfd] px-2.5 text-[13px] hover:bg-[#e5f1fb] disabled:opacity-40 disabled:hover:bg-[#fdfdfd] dark:border-white/30 dark:bg-transparent dark:hover:bg-white/10";

  async function save() {
    if (!sel) return;
    const h = editor?.getHtml() ?? html;
    list = list.map((s) => (s.name === sel ? { ...s, subject, html: h } : s));
    await saveTemplates(list);
    dirty = false;
  }

  async function confirmDiscard() {
    if (dirty && (await app.confirm(t("tpl.unsaved", sel), undefined, { ok: t("common.save"), cancel: t("sig.discard") }))) await save();
  }

  async function pick(name: string) {
    if (name === sel) return;
    await confirmDiscard();
    const s = list.find((x) => x.name === name);
    sel = name;
    subject = s?.subject ?? "";
    html = s?.html ?? "";
    dirty = false;
  }

  function uniqueOk(name: string, except = ""): boolean {
    if (!name.trim()) return false;
    if (list.some((s) => s.name.toLocaleLowerCase() === name.toLocaleLowerCase() && s.name !== except)) {
      app.error(t("tpl.exists", name));
      return false;
    }
    return true;
  }

  async function create() {
    await confirmDiscard();
    const name = (await app.prompt(t("tpl.new_title"), t("sig.name_label")))?.trim();
    if (!name || !uniqueOk(name)) return;
    list = [...list, { name, subject: "", html: "" }];
    await saveTemplates(list);
    sel = name;
    subject = "";
    html = "";
    dirty = false;
    setTimeout(() => editor?.focus(), 50);
  }

  async function rename() {
    if (!sel) return;
    const name = (await app.prompt(t("tpl.rename_title"), t("sig.name_label"), sel))?.trim();
    if (!name || name === sel || !uniqueOk(name, sel)) return;
    const old = sel;
    list = list.map((s) => (s.name === old ? { ...s, name } : s));
    await saveTemplates(list);
    sel = name;
  }

  async function remove() {
    if (!sel) return;
    if (!(await app.confirm(t("tpl.delete_confirm", sel), undefined, { ok: t("common.delete"), destructive: true }))) return;
    const old = sel;
    list = list.filter((s) => s.name !== old);
    await saveTemplates(list);
    sel = list[0]?.name ?? "";
    subject = list[0]?.subject ?? "";
    html = list[0]?.html ?? "";
    dirty = false;
  }

  // Ayarlar penceresi kapanırken kaydedilmemiş değişiklik kaybolmasın
  onDestroy(() => {
    if (dirty) void save();
  });
</script>

<div class="flex h-full min-h-0 flex-col gap-3 text-[13px]">
  <h3 class="text-lg font-semibold">{t("settings.tab_templates")}</h3>
  <p class="text-muted-foreground text-[12px]">{t("tpl.help")}</p>
  <div class="grid min-h-0 flex-1 gap-4 md:grid-cols-[200px_1fr]">
    <div class="flex min-h-0 flex-col gap-2">
      <div class="min-h-28 flex-1 overflow-y-auto rounded-[3px] border border-[#8a8a8a] bg-white dark:border-white/30 dark:bg-transparent" role="listbox">
        {#each list as s (s.name)}
          <button role="option" aria-selected={s.name === sel} class="block w-full truncate px-2 py-0.5 text-left {s.name === sel ? 'bg-primary text-primary-foreground' : 'hover:bg-[#e5f1fb] dark:hover:bg-white/10'}" onclick={() => pick(s.name)}>{s.name}</button>
        {:else}
          <p class="text-muted-foreground p-2 text-xs">{t("tpl.none")}</p>
        {/each}
      </div>
      <div class="flex flex-wrap gap-1.5">
        <button class={btn} onclick={create}>{t("sig.new")}</button>
        <button class={btn} disabled={!sel} onclick={remove}>{t("common.delete")}</button>
        <button class={btn} disabled={!sel} onclick={rename}>{t("sig.rename")}</button>
        <button class={btn} disabled={!sel || !dirty} onclick={save}>{t("common.save")}</button>
      </div>
    </div>
    <div class="flex min-h-0 flex-col gap-2 {sel ? '' : 'pointer-events-none opacity-50'}">
      <label class="flex items-center gap-2">
        <span class="w-14 shrink-0">{t("mail.subject")}:</span>
        <input class="h-[26px] min-w-0 flex-1 rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] outline-none focus:border-primary dark:border-white/30 dark:bg-transparent" bind:value={subject} oninput={() => (dirty = true)} />
      </label>
      <div class="flex min-h-[220px] flex-1 flex-col overflow-hidden rounded-[3px] border border-[#8a8a8a] bg-white dark:border-white/30 dark:bg-transparent">
        {#key sel}
          <RichEditor bind:this={editor} bind:html onchange={() => (dirty = true)} />
        {/key}
      </div>
    </div>
  </div>
</div>
