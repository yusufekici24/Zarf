<script lang="ts">
  // Güven Merkezi > Şifreleme: hesap başına S/MIME sertifikası, OpenPGP anahtarı ve varsayılanlar.
  // Yöntem, anahtar ve imza seçimleri diğer seçenekler gibi "Tamam" ile uygulanır; sertifika
  // içe aktarma ve kaldırma dosya işlemidir, hemen geçerli olur.
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import { open as openDialog } from "$lib/dialog";
  import { toast } from "svelte-sonner";
  import type { CertInfo, SecurityStatus } from "$lib/types";

  let { get, set }: { get: (key: string, def?: string) => string; set: (key: string, value: string) => void } = $props();

  let status = $state<SecurityStatus | null>(null);
  let certs = $state<Record<number, CertInfo | null>>({});
  const accounts = $derived(app.accounts.filter((a) => !a.shared_of));
  const selectClass = "h-[26px] w-72 rounded-[3px] border border-[#8a8a8a] bg-white px-1.5 text-[13px] dark:border-white/30 dark:bg-transparent";
  const buttonClass = "hover:bg-accent h-7 rounded border px-3";

  async function refresh() {
    status = await api.securityStatus();
    const found: Record<number, CertInfo | null> = {};
    for (const a of accounts) found[a.id] = await api.smimeIdentity(a.id);
    certs = found;
  }
  $effect(() => {
    void refresh().catch((e) => app.error(e));
  });

  async function importCert(id: number) {
    const path = await openDialog({ filters: [{ name: t("sec.p12_filter"), extensions: ["p12", "pfx"] }] });
    if (typeof path !== "string") return;
    const password = await app.prompt(t("sec.import_cert"), t("sec.p12_password"), "", true);
    if (password === null) return;
    try {
      certs = { ...certs, [id]: await api.smimeImport(id, path, password) };
    } catch (e) {
      app.error(e);
    }
  }

  async function removeCert(id: number) {
    if (!(await app.confirm(t("sec.remove_cert"), t("sec.remove_cert_text"), { ok: t("common.delete"), destructive: true }))) return;
    await api.smimeRemove(id);
    certs = { ...certs, [id]: null };
  }

  async function importPublic() {
    const path = await openDialog({ filters: [{ name: t("sec.public_filter"), extensions: ["pem", "crt", "cer", "asc", "gpg", "pgp", "key"] }] });
    if (typeof path !== "string") return;
    try {
      const added = await api.importPublicKey(path);
      toast.success(added.length ? t("sec.imported_for", added.join(", ")) : t("sec.import_done"));
      await refresh();
    } catch (e) {
      app.error(e);
    }
  }
</script>

<div class="max-w-xl space-y-2 py-1 pl-4 text-[13px]">
  <p class="text-muted-foreground text-xs">{t("sec.tools")}</p>
  {#if status}
    <div class="text-xs">
      GnuPG (OpenPGP): {status.gpg ? t("sec.installed") : t("sec.not_installed", "sudo apt install gnupg")}<br />
      OpenSSL (S/MIME): {status.openssl ? t("sec.installed") : t("sec.not_installed", "sudo apt install openssl")}
    </div>
  {/if}
  {#each accounts as a (a.id)}
    <div class="space-y-1 rounded-[3px] border border-[#8a8a8a] p-2 dark:border-white/30">
      <div class="font-medium">{a.email}</div>
      <div class="flex items-center gap-2">
        <span class="w-36 shrink-0">{t("sec.method")}:</span>
        <select class={selectClass} value={get(`security.method.${a.id}`, "smime")} onchange={(e) => set(`security.method.${a.id}`, (e.target as HTMLSelectElement).value)}>
          <option value="smime">S/MIME</option>
          <option value="pgp">OpenPGP</option>
        </select>
      </div>
      <div class="flex items-center gap-2">
        <span class="w-36 shrink-0">{t("sec.pgp_key")}:</span>
        <select class={selectClass} value={get(`security.pgp_key.${a.id}`, "")} onchange={(e) => set(`security.pgp_key.${a.id}`, (e.target as HTMLSelectElement).value)}>
          <option value="">{t("sec.pgp_default")}</option>
          {#each status?.keys ?? [] as k (k.fingerprint)}<option value={k.fingerprint}>{k.uid}</option>{/each}
        </select>
      </div>
      <div class="flex items-center gap-2">
        <span class="w-36 shrink-0">{t("sec.smime_cert")}:</span>
        <span class="selectable min-w-0 flex-1 truncate" title={certs[a.id]?.subject}>{#if certs[a.id]}{certs[a.id]?.emails.join(", ") || certs[a.id]?.subject} ({certs[a.id]?.expires}){:else}{t("sec.cert_none")}{/if}</span>
      </div>
      <div class="flex items-center gap-2">
        <span class="w-36 shrink-0"></span>
        <button class={buttonClass} onclick={() => importCert(a.id)}>{t("sec.import_cert")}</button>
        {#if certs[a.id]}<button class={buttonClass} onclick={() => removeCert(a.id)}>{t("sec.remove_cert")}</button>{/if}
      </div>
      <label class="flex items-center gap-2 py-[3px]">
        <input type="checkbox" class="accent-primary size-[15px]" checked={get(`security.sign.${a.id}`, "0") === "1"} onchange={(e) => set(`security.sign.${a.id}`, (e.target as HTMLInputElement).checked ? "1" : "0")} />
        {t("sec.sign_default")}
      </label>
    </div>
  {/each}
  <button class={buttonClass} onclick={importPublic}>{t("sec.import_public")}</button>
</div>
