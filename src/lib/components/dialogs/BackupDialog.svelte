<script lang="ts">
  // Yedekle / geri yükle. Yedek tek dosyadır; parola verilirse şifrelenir. Geri yükleme önce dosyayı
  // doğrulayıp özetini gösterir, onaylanınca uygulama yeniden açılırken yerine konur.
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { toast } from "svelte-sonner";
  import { api, errorText, type BackupInfo } from "$lib/api";
  import { open as openDialog, save as saveDialog } from "$lib/dialog";
  import { app } from "$lib/stores/app.svelte";
  import { fmtDateTime } from "$lib/format";
  import { t } from "$lib/i18n.svelte";

  let { mode, onclose }: { mode: "backup" | "restore"; onclose: () => void } = $props();

  const FILTER = [{ name: t("backup.filter"), extensions: ["mnbak"] }];
  let path = $state("");
  let password = $state("");
  let confirmPassword = $state("");
  let encrypted = $state(false);
  let busy = $state(false);
  let error = $state("");
  /** Doğrulanmış yedeğin özeti: onay bekleniyor */
  let info = $state<BackupInfo | null>(null);

  const canStart = $derived(!!path && !busy && (mode === "backup" ? password === confirmPassword : !encrypted || !!password));

  async function pickFile() {
    error = "";
    if (mode === "backup") {
      const stamp = new Date().toISOString().slice(0, 10);
      path = (await saveDialog({ defaultPath: `mailnix-${stamp}.mnbak`, filters: FILTER })) ?? path;
      return;
    }
    const r = await openDialog({ filters: FILTER });
    if (typeof r !== "string") return;
    try {
      encrypted = await api.backupIsEncrypted(r);
      path = r;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function start() {
    busy = true;
    error = "";
    try {
      if (mode === "backup") {
        const r = await api.createBackup(path, password);
        toast.success(t("backup.done"), { description: summary(r) });
        onclose();
      } else {
        info = await api.prepareRestore(path, password);
      }
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  function summary(r: BackupInfo): string {
    return t("backup.summary", r.counts.messages ?? 0, r.counts.contacts ?? 0, r.counts.events ?? 0);
  }

  async function restore() {
    busy = true;
    try {
      await api.finishRestore();
    } catch (e) {
      error = errorText(e);
      busy = false;
    }
  }

  async function cancel() {
    if (info) await api.cancelRestore().catch(() => {});
    onclose();
  }
</script>

<Dialog.Root open onOpenChange={(o) => { if (!o) void cancel(); }}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{mode === "backup" ? t("backup.create_title") : t("backup.restore_title")}</Dialog.Title>
      <Dialog.Description>{mode === "backup" ? t("backup.create_help") : t("backup.restore_help")}</Dialog.Description>
    </Dialog.Header>
    {#if info}
      <div class="space-y-1 rounded-md border p-3 text-sm">
        <div><span class="text-muted-foreground">{t("backup.created")}:</span> {fmtDateTime(new Date(info.created).getTime() / 1000)}</div>
        <div><span class="text-muted-foreground">{t("backup.accounts")}:</span> {info.accounts.join(", ") || "-"}</div>
        <div>{summary(info)}</div>
      </div>
      <p class="text-destructive text-sm">{t("backup.restore_warning")}</p>
    {:else}
      <div class="space-y-3 text-sm">
        <div class="flex items-center gap-2">
          <Input class="min-w-0 flex-1" readonly value={path} placeholder={t("backup.no_file")} />
          <Button variant="outline" onclick={pickFile}>{t("backup.choose")}</Button>
        </div>
        {#if mode === "backup"}
          <Input type="password" autocomplete="new-password" placeholder={t("backup.password_optional")} bind:value={password} />
          <Input type="password" autocomplete="new-password" placeholder={t("backup.password_again")} bind:value={confirmPassword} />
          {#if password && password !== confirmPassword}<p class="text-destructive text-xs">{t("backup.password_mismatch")}</p>{/if}
          {#if password}<p class="text-muted-foreground text-xs">{t("backup.password_warning")}</p>{/if}
        {:else if encrypted}
          <Input type="password" autocomplete="current-password" placeholder={t("backup.password")} bind:value={password} />
        {/if}
      </div>
    {/if}
    {#if error}<p class="text-destructive text-sm">{error}</p>{/if}
    <Dialog.Footer>
      <Button variant="outline" disabled={busy} onclick={cancel}>{t("common.cancel")}</Button>
      {#if info}
        <Button variant="destructive" disabled={busy} onclick={restore}>{t("backup.restore_restart")}</Button>
      {:else}
        <Button disabled={!canStart} onclick={start}>{busy ? t("common.loading") : mode === "backup" ? t("backup.create") : t("common.continue")}</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
