<script lang="ts">
  // Kimlikler (takma adlar): hesabın ek "Kimden" adresleri. Yeni iletideki Kimden menüsünde
  // hesabın altında görünür; takma ada gelen iletiye yanıt verilirken kendiliğinden seçilir.
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { loadSignatures } from "$lib/signatures";
  import { loadIdentities, saveIdentities, type Identity } from "$lib/identities";

  let list = $state<Identity[]>(loadIdentities());
  let account = $state<number | null>(app.accounts[0]?.id ?? null);
  let editing = $state<Identity | null>(null);
  /** Düzenlemeye açılan kaydın eski adresi (null: yeni kayıt) */
  let original = $state<string | null>(null);

  const rows = $derived(list.filter((x) => x.account_id === account));
  const btn = "h-[26px] rounded-[3px] border border-[#8a8a8a] bg-[#fdfdfd] px-2.5 text-[13px] hover:bg-[#e5f1fb] disabled:opacity-40 disabled:hover:bg-[#fdfdfd] dark:border-white/30 dark:bg-transparent dark:hover:bg-white/10";
  const input = "h-[26px] w-full rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] outline-none focus:border-primary dark:border-white/30 dark:bg-transparent";

  function add() {
    if (account == null) return;
    original = null;
    editing = { account_id: account, email: "", name: app.accounts.find((a) => a.id === account)?.display_name ?? "", reply_to: "", signature: "" };
  }

  function edit(x: Identity) {
    original = x.email;
    editing = { ...x };
  }

  async function commit() {
    const e = editing;
    if (!e) return;
    const email = e.email.trim();
    if (!/^[^\s@<>]+@[^\s@<>]+\.[^\s@<>]+$/.test(email)) {
      app.error(t("identity.bad_email"));
      return;
    }
    const same = (x: Identity, addr: string) => x.account_id === e.account_id && x.email.toLowerCase() === addr.toLowerCase();
    if (email.toLowerCase() !== original?.toLowerCase() && list.some((x) => same(x, email))) {
      app.error(t("identity.exists", email));
      return;
    }
    const rest = original == null ? list : list.filter((x) => !same(x, original!));
    list = [...rest, { ...e, email }];
    await saveIdentities(list);
    editing = null;
  }

  async function remove(x: Identity) {
    if (!(await app.confirm(t("identity.delete_confirm", x.email), undefined, { ok: t("common.delete"), destructive: true }))) return;
    list = list.filter((y) => y !== x);
    await saveIdentities(list);
  }
</script>

<div class="space-y-3 text-[13px]">
  <h3 class="text-lg font-semibold">{t("settings.tab_identities")}</h3>
  <p class="text-muted-foreground text-[12px]">{t("identity.help")}</p>
  {#if app.accounts.length}
    <label class="flex items-center gap-2">
      <span class="w-24 shrink-0">{t("sig.account")}:</span>
      <select class={input} value={account} onchange={(e) => { account = Number((e.target as HTMLSelectElement).value); editing = null; }}>
        {#each app.accounts as a (a.id)}<option value={a.id}>{a.email}</option>{/each}
      </select>
    </label>
    <div class="min-h-24 rounded-[3px] border border-[#8a8a8a] bg-white dark:border-white/30 dark:bg-transparent">
      {#each rows as x (x.email)}
        <div class="flex items-center gap-2 border-b px-2 py-1 last:border-b-0">
          <span class="min-w-0 flex-1 truncate">{x.name ? `${x.name} <${x.email}>` : x.email}</span>
          <button class={btn} onclick={() => edit(x)}>{t("common.edit")}</button>
          <button class={btn} onclick={() => remove(x)}>{t("common.delete")}</button>
        </div>
      {:else}
        <p class="text-muted-foreground p-2 text-xs">{t("identity.none")}</p>
      {/each}
    </div>
    {#if editing}
      <fieldset class="grid grid-cols-[110px_1fr] items-center gap-x-2 gap-y-2 rounded-[3px] border border-black/15 px-3 pt-1 pb-3 dark:border-white/20">
        <legend class="px-1">{original == null ? t("identity.add") : original}</legend>
        <span>{t("identity.email")}:</span>
        <input class={input} bind:value={editing.email} />
        <span>{t("identity.name")}:</span>
        <input class={input} bind:value={editing.name} />
        <span>{t("identity.reply_to")}:</span>
        <input class={input} bind:value={editing.reply_to} />
        <span>{t("compose.signature")}:</span>
        <select class={input} bind:value={editing.signature}>
          <option value="">{t("identity.account_signature")}</option>
          {#each loadSignatures() as s (s.name)}<option value={s.name}>{s.name}</option>{/each}
        </select>
        <div class="col-span-2 flex justify-end gap-1.5">
          <button class={btn} onclick={() => (editing = null)}>{t("common.cancel")}</button>
          <button class={btn} onclick={commit}>{t("common.save")}</button>
        </div>
      </fieldset>
    {:else}
      <button class={btn} onclick={add}>{t("identity.add")}</button>
    {/if}
  {:else}
    <p class="text-muted-foreground">{t("account.none")}</p>
  {/if}
</div>
