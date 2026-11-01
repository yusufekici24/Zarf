<script lang="ts">
  // Exchange kuralı düzenleyicisi — Outlook "Kural Sihirbazı" gibi: birden çok koşul,
  // eylem ve özel durum; her biri işaretlenip değeri girilir. Sunucudaki karmaşık
  // kurallar da düzenlenebilir (modelde olmayan öğeler korunur).
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Switch } from "$lib/components/ui/switch";
  import AddressInput from "$lib/components/common/AddressInput.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { splitAddresses } from "$lib/format";
  import { untrack } from "svelte";
  import type { RulePart, RuleSpec } from "$lib/types";

  let {
    open = $bindable(false),
    ruleId = 0,
    accountId,
    name: initialName = "",
    enabled: initialEnabled = true,
    prefillFrom = "",
    onsaved,
  }: {
    open?: boolean;
    ruleId?: number;
    accountId: number;
    name?: string;
    enabled?: boolean;
    prefillFrom?: string;
    onsaved: () => void;
  } = $props();

  type Kind = "bool" | "strings" | "addresses" | "folder" | "importance" | "sensitivity" | "categories" | "size";

  // Outlook Kural Sihirbazı'ndaki sırayla koşullar
  const CONDITIONS: [string, Kind][] = [
    ["from", "addresses"],
    ["sent_to", "addresses"],
    ["sent_only_to_me", "bool"],
    ["sent_to_me", "bool"],
    ["sent_cc_me", "bool"],
    ["sent_to_or_cc_me", "bool"],
    ["not_sent_to_me", "bool"],
    ["subject", "strings"],
    ["body", "strings"],
    ["subject_or_body", "strings"],
    ["header", "strings"],
    ["sender_contains", "strings"],
    ["recipient_contains", "strings"],
    ["importance", "importance"],
    ["sensitivity", "sensitivity"],
    ["categories", "categories"],
    ["has_attachments", "bool"],
    ["size_range", "size"],
    ["is_meeting_request", "bool"],
    ["is_meeting_response", "bool"],
    ["is_automatic_reply", "bool"],
    ["is_automatic_forward", "bool"],
    ["is_read_receipt", "bool"],
    ["is_ndr", "bool"],
    ["is_encrypted", "bool"],
    ["is_signed", "bool"],
  ];
  const ACTIONS: [string, Kind][] = [
    ["move", "folder"],
    ["copy", "folder"],
    ["delete", "bool"],
    ["permanent_delete", "bool"],
    ["forward", "addresses"],
    ["forward_attachment", "addresses"],
    ["redirect", "addresses"],
    ["mark_read", "bool"],
    ["importance", "importance"],
    ["categories", "categories"],
    ["stop", "bool"],
  ];

  type Row = { on: boolean; values: string[] };
  type Section = Record<string, Row>;

  let ruleName = $state("");
  let isEnabled = $state(true);
  let cond = $state<Section>({});
  let exc = $state<Section>({});
  let act = $state<Section>({});
  /** Modelde olmayan (korunacak) öğeler */
  let raw = $state<{ conditions: RulePart[]; exceptions: RulePart[]; actions: RulePart[] }>({ conditions: [], exceptions: [], actions: [] });
  let showExceptions = $state(false);
  let saving = $state(false);
  let loading = $state(false);
  let loadError = $state("");

  function blank(list: [string, Kind][]): Section {
    return Object.fromEntries(list.map(([k]) => [k, { on: false, values: [] }]));
  }

  function fill(sec: Section, parts: RulePart[], rawOut: RulePart[]) {
    for (const p of parts) {
      if (p.kind === "raw" || !sec[p.kind]) rawOut.push(p);
      else sec[p.kind] = { on: true, values: [...p.values] };
    }
  }

  // Açılınca bir kez doldurulur (yalnızca open/ruleId izlenir; aşağıdaki yazmalar
  // effect'i yeniden tetiklemesin — "Kural oluştur"da sonsuz döngüye girip boş kalıyordu)
  $effect(() => {
    if (!open) return;
    void ruleId;
    untrack(init);
  });

  function init() {
    ruleName = initialName;
    isEnabled = initialEnabled;
    cond = blank(CONDITIONS);
    exc = blank(CONDITIONS);
    act = blank(ACTIONS);
    raw = { conditions: [], exceptions: [], actions: [] };
    showExceptions = false;
    loadError = "";
    if (ruleId > 0) {
      loading = true;
      api
        .serverRuleSpec(ruleId)
        .then((spec) => {
          const r = { conditions: [] as RulePart[], exceptions: [] as RulePart[], actions: [] as RulePart[] };
          fill(cond, spec.conditions, r.conditions);
          fill(exc, spec.exceptions, r.exceptions);
          fill(act, spec.actions, r.actions);
          raw = r;
          showExceptions = spec.exceptions.length > 0;
        })
        .catch((e) => (loadError = String(e)))
        .finally(() => (loading = false));
    } else if (prefillFrom) {
      cond.from = { on: true, values: [prefillFrom] };
    }
  }

  const folders = $derived(
    (app.folderGroups.find((g) => g.account?.id === accountId)?.folders ?? []).filter((f) => f.remote_name),
  );

  function folderLabel(id: string): string {
    return folders.find((f) => f.remote_name === id)?.display ?? t("rules.choose_folder");
  }

  function toSpec(sec: Section, list: [string, Kind][], rawParts: RulePart[]): RulePart[] {
    const out: RulePart[] = [];
    for (const [k, kind] of list) {
      const r = sec[k];
      if (!r?.on) continue;
      let values = r.values;
      if (kind === "addresses") values = splitAddresses(values.join(", "));
      if (kind === "strings" || kind === "categories") values = values.flatMap((v) => v.split(/[;\n]/)).map((v) => v.trim()).filter(Boolean);
      out.push({ kind: k, values });
    }
    return [...out, ...rawParts];
  }

  function summary(): string {
    const parts = CONDITIONS.filter(([k]) => cond[k]?.on).map(([k]) => t(`rw.c_${k}`));
    const acts = ACTIONS.filter(([k]) => act[k]?.on && k !== "stop").map(([k]) => t(`rw.a_${k}`));
    return `${parts.join(", ") || t("rw.all_messages")} → ${acts.join(", ")}`.slice(0, 120);
  }

  async function save(force = false) {
    const spec: RuleSpec = {
      conditions: toSpec(cond, CONDITIONS, raw.conditions),
      exceptions: toSpec(exc, CONDITIONS, raw.exceptions),
      actions: toSpec(act, ACTIONS, raw.actions),
    };
    if (!spec.actions.some((a) => a.kind !== "stop")) {
      app.error(t("rules.need_action"));
      return;
    }
    const name = ruleName.trim() || summary();
    saving = true;
    try {
      await api.saveServerRule(ruleId, accountId, name, isEnabled, spec, force);
      open = false;
      onsaved();
    } catch (e) {
      const msg = String(e);
      if (msg.includes("rules.blob_exists")) {
        if (await app.confirm(t("rules.blob_title"), t("rules.blob_text"), { ok: t("common.continue"), destructive: true })) await save(true);
      } else if (msg.startsWith("rules.need_value:")) {
        const k = msg.split(":")[1];
        app.error(t("rules.need_value", t(`rw.c_${k}`) !== `rw.c_${k}` ? t(`rw.c_${k}`) : t(`rw.a_${k}`)));
      } else app.error(e);
    } finally {
      saving = false;
    }
  }
</script>

{#snippet editor(sec: Section, k: string, kind: Kind)}
  {@const r = sec[k]}
  {#if r?.on && kind !== "bool"}
    <div class="mt-1 mb-2 ml-7">
      {#if kind === "addresses"}
        <div class="border-input flex max-h-24 min-h-9 items-center overflow-y-auto rounded-md border px-2">
          <AddressInput value={r.values.join(", ")} onvalue={(v) => (r.values = [v])} />
        </div>
      {:else if kind === "strings"}
        <Input class="h-8" placeholder={t("rw.words_hint")} value={r.values.join("; ")} oninput={(e) => (r.values = [(e.target as HTMLInputElement).value])} />
      {:else if kind === "folder"}
        <select class="border-input h-8 w-full rounded-md border bg-transparent px-2 text-sm" value={r.values[0] ?? ""} onchange={(e) => (r.values = [(e.target as HTMLSelectElement).value])}>
          <option value="" disabled>{t("rules.choose_folder")}</option>
          {#if r.values[0] && !folders.some((f) => f.remote_name === r.values[0])}<option value={r.values[0]}>{folderLabel(r.values[0])}</option>{/if}
          {#each folders as f (f.id)}<option value={f.remote_name}>{f.display}</option>{/each}
        </select>
      {:else if kind === "importance"}
        <select class="border-input h-8 rounded-md border bg-transparent px-2 text-sm" value={r.values[0] ?? "High"} onchange={(e) => (r.values = [(e.target as HTMLSelectElement).value])}>
          <option value="High">{t("importance.high")}</option>
          <option value="Normal">{t("importance.normal")}</option>
          <option value="Low">{t("importance.low")}</option>
        </select>
      {:else if kind === "sensitivity"}
        <select class="border-input h-8 rounded-md border bg-transparent px-2 text-sm" value={r.values[0] ?? "Normal"} onchange={(e) => (r.values = [(e.target as HTMLSelectElement).value])}>
          {#each ["Normal", "Personal", "Private", "Confidential"] as v (v)}<option value={v}>{t(`rw.sens_${v.toLowerCase()}`)}</option>{/each}
        </select>
      {:else if kind === "categories"}
        <div class="flex flex-wrap gap-1">
          {#each app.categories as c (c.name)}
            {@const on = r.values.includes(c.name)}
            <button type="button" class="flex items-center gap-1 rounded border px-2 py-0.5 text-xs {on ? 'bg-accent font-medium' : ''}" onclick={() => (r.values = on ? r.values.filter((x) => x !== c.name) : [...r.values, c.name])}>
              <span class="size-2.5 rounded-sm" style="background:{c.color}"></span>{c.name}
            </button>
          {/each}
        </div>
      {:else if kind === "size"}
        <div class="flex items-center gap-2 text-sm">
          <span>{t("rw.size_min")}</span><Input class="h-8 w-24" type="number" min="0" value={r.values[0] ?? ""} oninput={(e) => (r.values = [(e.target as HTMLInputElement).value, r.values[1] ?? ""])} />
          <span>{t("rw.size_max")}</span><Input class="h-8 w-24" type="number" min="0" value={r.values[1] ?? ""} oninput={(e) => (r.values = [r.values[0] ?? "", (e.target as HTMLInputElement).value])} />
          <span class="text-muted-foreground">KB</span>
        </div>
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet section(sec: Section, list: [string, Kind][], prefix: string)}
  <div class="max-h-72 overflow-y-auto rounded-md border p-2">
    {#each list as [k, kind] (k)}
      <label class="hover:bg-accent/50 flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 text-sm">
        <input type="checkbox" checked={sec[k]?.on} onchange={(e) => (sec[k] = { on: (e.target as HTMLInputElement).checked, values: sec[k]?.values ?? [] })} />
        <span>{t(`rw.${prefix}_${k}`)}</span>
      </label>
      {@render editor(sec, k, kind)}
    {/each}
  </div>
{/snippet}

<Dialog.Root bind:open>
  <Dialog.Content class="z-[70] flex max-h-[92vh] flex-col overflow-hidden sm:max-w-2xl" overlayClass="z-[70]">
    <Dialog.Header><Dialog.Title>{ruleId ? t("rw.edit_title") : t("rw.new_title")}</Dialog.Title></Dialog.Header>
    {#if loading}
      <div class="text-muted-foreground p-6 text-center text-sm">{t("common.loading")}</div>
    {:else if loadError}
      <div class="text-destructive p-6 text-center text-sm">{loadError}</div>
    {:else}
      <div class="min-h-0 shrink space-y-3 overflow-y-auto pr-1">
        <div class="flex items-center gap-3">
          <Input placeholder={t("rules.name")} bind:value={ruleName} />
          <label class="flex shrink-0 items-center gap-2 text-sm"><Switch bind:checked={isEnabled} />{t("rw.enabled")}</label>
        </div>
        <div>
          <div class="mb-1 text-sm font-semibold">{t("rw.step_conditions")}</div>
          {@render section(cond, CONDITIONS, "c")}
          {#if raw.conditions.length}<p class="text-muted-foreground mt-1 text-xs">{t("rw.preserved", raw.conditions.length)}</p>{/if}
        </div>
        <div>
          <div class="mb-1 text-sm font-semibold">{t("rw.step_actions")}</div>
          {@render section(act, ACTIONS, "a")}
          {#if raw.actions.length}<p class="text-muted-foreground mt-1 text-xs">{t("rw.preserved", raw.actions.length)}</p>{/if}
        </div>
        <div>
          <button class="text-primary text-sm font-semibold hover:underline" onclick={() => (showExceptions = !showExceptions)}>{t("rw.step_exceptions")} {showExceptions ? "▴" : "▾"}</button>
          {#if showExceptions}
            <div class="mt-1">{@render section(exc, CONDITIONS, "c")}</div>
          {/if}
        </div>
      </div>
    {/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (open = false)}>{t("common.cancel")}</Button>
      <Button data-default-button disabled={saving || loading || !!loadError} onclick={() => save()}>{saving ? t("common.saving") : t("common.save")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
