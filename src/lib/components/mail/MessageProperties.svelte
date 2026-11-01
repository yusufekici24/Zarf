<script lang="ts">
  // İleti "Özellikler" penceresi (Dosya > Bilgi > Özellikler): önem derecesi, kategoriler,
  // boyut ve İnternet üst bilgileri.
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import type { MessageView } from "$lib/types";

  let { message, open = $bindable(false), onchange = undefined }: { message: MessageView; open?: boolean; onchange?: () => void } = $props();

  let headers = $state("");
  let size = $state("");
  let importance = $state(1);

  $effect(() => {
    if (!open) return;
    importance = message.importance ?? 1;
    headers = "";
    size = "…";
    void messageSource(message.id).then((src) => {
      size = sizeText(new TextEncoder().encode(src).length);
      const end = src.search(/\r?\n\r?\n/);
      headers = (end > 0 ? src.slice(0, end) : src).replace(/\r\n/g, "\n");
    });
  });

  async function save() {
    try {
      if (!message.external && importance !== (message.importance ?? 1)) await api.setImportance([message.id], importance);
      onchange?.();
    } catch (e) {
      app.error(e);
    }
    open = false;
  }
</script>

<script lang="ts" module>
  import { invoke } from "@tauri-apps/api/core";
  import { api as api2 } from "$lib/api";

  /** İletinin ham (RFC 822) içeriği: geçici dosyaya kaydedilip okunur */
  export async function messageSource(id: number): Promise<string> {
    try {
      const tmp = `${await import("@tauri-apps/api/path").then((p) => p.tempDir())}/mailnix-src-${id}.eml`;
      await api2.saveMessageAs(id, tmp);
      return await invoke<string>("read_text_file", { path: tmp });
    } catch {
      return "";
    }
  }

  /** Boyut metni (Outlook gibi KB) */
  export function sizeText(bytes: number): string {
    return bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{t("props.title")}</Dialog.Title>
      <Dialog.Description class="truncate">{message.subject || t("mail.no_subject")}</Dialog.Description>
    </Dialog.Header>
    <div class="grid grid-cols-[140px_1fr] items-center gap-x-3 gap-y-2 text-sm">
      <span class="text-muted-foreground">{t("mail.importance")}</span>
      <select class="bg-background h-8 w-48 rounded border px-2" bind:value={importance} disabled={message.external}>
        <option value={0}>{t("importance.low")}</option>
        <option value={1}>{t("importance.normal")}</option>
        <option value={2}>{t("importance.high")}</option>
      </select>
      <span class="text-muted-foreground">{t("mail.categorize")}</span>
      <span>{message.categories || "—"}</span>
      <span class="text-muted-foreground">{t("props.size")}</span>
      <span>{size}</span>
    </div>
    <div class="mt-2">
      <p class="text-muted-foreground mb-1 text-sm">{t("props.headers")}</p>
      <textarea class="bg-background h-56 w-full resize-none rounded border p-2 font-mono text-xs" readonly value={headers}></textarea>
    </div>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (open = false)}>{t("common.cancel")}</Button>
      <Button onclick={save}>{t("common.ok")}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
