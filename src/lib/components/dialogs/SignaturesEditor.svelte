<script lang="ts">
  // Outlook "İmzalar ve Kırtasiye": düzenlenecek imzayı seç (Sil / Yeni / Kaydet / Yeniden
  // Adlandır), her hesap için varsayılan imza (yeni iletiler, yanıtlar/iletmeler) ve biçimli
  // düzenleyici. İmzalar yalnızca bu bilgisayarda saklanır (sunucuya gönderilmez).
  import RichEditor from "$lib/components/common/RichEditor.svelte";
  import { onDestroy } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { loadSignatures, saveSignatures, defaultSignature, type Signature } from "$lib/signatures";

  let list = $state<Signature[]>(loadSignatures());
  let sel = $state<string>(list[0]?.name ?? "");
  let html = $state(list[0]?.html ?? "");
  let dirty = $state(false);
  let editor: RichEditor | undefined = $state();
  let account = $state<number | null>(app.accounts[0]?.id ?? null);
  let defNew = $state("");
  let defReply = $state("");

  $effect(() => {
    const a = account;
    defNew = defaultSignature(a, "new");
    defReply = defaultSignature(a, "reply");
  });

  const btn = "h-[26px] rounded-[3px] border border-[#8a8a8a] bg-[#fdfdfd] px-2.5 text-[13px] hover:bg-(--primary-hover) disabled:opacity-40 disabled:hover:bg-[#fdfdfd] dark:border-white/30 dark:bg-transparent dark:hover:bg-white/10";
  const selCls = "h-[26px] w-full rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] dark:border-white/30 dark:bg-transparent";

  async function confirmDiscard(): Promise<boolean> {
    if (!dirty) return true;
    const r = await app.confirm(t("sig.unsaved", sel), undefined, { ok: t("common.save"), cancel: t("sig.discard") });
    if (r) await save();
    return true;
  }

  async function pick(name: string) {
    if (name === sel) return;
    await confirmDiscard();
    sel = name;
    html = list.find((s) => s.name === name)?.html ?? "";
    dirty = false;
  }

  async function save() {
    if (!sel) return;
    const h = editor?.getHtml() ?? html;
    list = list.map((s) => (s.name === sel ? { ...s, html: h } : s));
    await saveSignatures(list);
    dirty = false;
  }

  function uniqueOk(name: string, except = ""): boolean {
    if (!name.trim()) return false;
    if (list.some((s) => s.name.toLocaleLowerCase() === name.toLocaleLowerCase() && s.name !== except)) {
      app.error(t("sig.exists", name));
      return false;
    }
    return true;
  }

  async function create() {
    await confirmDiscard();
    const name = (await app.prompt(t("sig.new_title"), t("sig.name_label")))?.trim();
    if (!name || !uniqueOk(name)) return;
    list = [...list, { name, html: "" }];
    await saveSignatures(list);
    sel = name;
    html = "";
    dirty = false;
    setTimeout(() => editor?.focus(), 50);
  }

  async function rename() {
    if (!sel) return;
    const name = (await app.prompt(t("sig.rename_title"), t("sig.name_label"), sel))?.trim();
    if (!name || name === sel || !uniqueOk(name, sel)) return;
    const old = sel;
    list = list.map((s) => (s.name === old ? { ...s, name } : s));
    await saveSignatures(list);
    // Varsayılan olarak seçildiği hesaplarda da adı güncellenir
    for (const a of app.accounts) {
      for (const k of ["new", "reply"] as const) if (defaultSignature(a.id, k) === old) await app.setSetting(`sig_${k}_${a.id}`, name);
    }
    sel = name;
    defNew = defaultSignature(account, "new");
    defReply = defaultSignature(account, "reply");
  }

  async function remove() {
    if (!sel) return;
    if (!(await app.confirm(t("sig.delete_confirm", sel), undefined, { ok: t("common.delete"), destructive: true }))) return;
    const old = sel;
    list = list.filter((s) => s.name !== old);
    await saveSignatures(list);
    for (const a of app.accounts) {
      for (const k of ["new", "reply"] as const) if (defaultSignature(a.id, k) === old) await app.setSetting(`sig_${k}_${a.id}`, "");
    }
    sel = list[0]?.name ?? "";
    html = list[0]?.html ?? "";
    dirty = false;
    defNew = defaultSignature(account, "new");
    defReply = defaultSignature(account, "reply");
  }

  // Ayarlar penceresi kapanırken kaydedilmemiş değişiklik kaybolmasın
  onDestroy(() => {
    if (dirty) void save();
  });

  async function setDefault(kind: "new" | "reply", name: string) {
    if (account == null) return;
    await app.setSetting(`sig_${kind}_${account}`, name);
    if (kind === "new") defNew = name;
    else defReply = name;
  }
</script>

<div class="flex h-full min-h-0 flex-col gap-3 text-[13px]">
  <h3 class="text-lg font-semibold">{t("sig.title")}</h3>
  <div class="grid gap-4 md:grid-cols-[1fr_1fr]">
    <!-- Düzenlenecek imza -->
    <fieldset class="min-w-0 rounded-[3px] border border-black/15 px-3 pt-1 pb-3 dark:border-white/20">
      <legend class="px-1">{t("sig.select_edit")}</legend>
      <div class="h-28 overflow-y-auto rounded-[3px] border border-[#8a8a8a] bg-white dark:border-white/30 dark:bg-transparent" role="listbox">
        {#each list as s (s.name)}
          <button role="option" aria-selected={s.name === sel} class="block w-full truncate px-2 py-0.5 text-left {s.name === sel ? 'bg-primary text-primary-foreground' : 'hover:bg-(--primary-hover) dark:hover:bg-white/10'}" onclick={() => pick(s.name)}>{s.name}</button>
        {:else}
          <p class="text-muted-foreground p-2 text-xs">{t("sig.none")}</p>
        {/each}
      </div>
      <div class="mt-2 flex flex-wrap gap-1.5">
        <button class={btn} disabled={!sel} onclick={remove}>{t("common.delete")}</button>
        <button class={btn} onclick={create}>{t("sig.new")}</button>
        <button class={btn} disabled={!sel || !dirty} onclick={save}>{t("common.save")}</button>
        <button class={btn} disabled={!sel} onclick={rename}>{t("sig.rename")}</button>
      </div>
    </fieldset>
    <!-- Varsayılan imza -->
    <fieldset class="min-w-0 rounded-[3px] border border-black/15 px-3 pt-1 pb-3 dark:border-white/20">
      <legend class="px-1">{t("sig.choose_default")}</legend>
      <div class="grid grid-cols-[130px_1fr] items-center gap-x-2 gap-y-2">
        <span>{t("sig.account")}:</span>
        <select class={selCls} value={account} onchange={(e) => (account = Number((e.target as HTMLSelectElement).value))}>
          {#each app.accounts as a (a.id)}<option value={a.id}>{a.email}</option>{/each}
        </select>
        <span>{t("sig.new_messages")}:</span>
        <select class={selCls} value={defNew} disabled={account == null} onchange={(e) => setDefault("new", (e.target as HTMLSelectElement).value)}>
          <option value="">{t("sig.none_option")}</option>
          {#each list as s (s.name)}<option value={s.name}>{s.name}</option>{/each}
        </select>
        <span>{t("sig.replies")}:</span>
        <select class={selCls} value={defReply} disabled={account == null} onchange={(e) => setDefault("reply", (e.target as HTMLSelectElement).value)}>
          <option value="">{t("sig.none_option")}</option>
          {#each list as s (s.name)}<option value={s.name}>{s.name}</option>{/each}
        </select>
      </div>
      <p class="text-muted-foreground mt-3 text-[12px]">{t("sig.local_note")}</p>
    </fieldset>
  </div>
  <!-- İmzayı düzenle -->
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="mb-1">{t("sig.edit")}</div>
    <div class="flex min-h-[220px] flex-1 flex-col overflow-hidden rounded-[3px] border border-[#8a8a8a] bg-white dark:border-white/30 dark:bg-transparent {sel ? '' : 'pointer-events-none opacity-50'}">
      {#key sel}
        <RichEditor bind:this={editor} bind:html onchange={() => (dirty = true)} />
      {/key}
    </div>
  </div>
</div>
