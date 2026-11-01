<script lang="ts">
  // Genel onay ve metin giriş iletişim kutuları (app.confirm / app.prompt).
  import * as AlertDialog from "$lib/components/ui/alert-dialog";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import CircleHelp from "@lucide/svelte/icons/circle-help";
  import Info from "@lucide/svelte/icons/info";
  import X from "@lucide/svelte/icons/x";

  let promptValue = $state("");
  $effect(() => {
    if (app.promptReq) promptValue = app.promptReq.value;
  });

  function closeConfirm(v: boolean | "alt") {
    const req = app.confirmReq;
    app.confirmReq = null;
    if (!req) return;
    // Onay penceresi animasyonsuz anında kapanır (app.css); sonuç ilk karede bildirilir:
    // ardından gelen ağır iş (ör. çok sayıda iletiyi silmek) pencereyi önde bekletmez
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      req.resolve(v);
    };
    // (kapanış karesi çizildikten hemen sonra: ~1 kare)
    requestAnimationFrame(() => setTimeout(finish, 0));
    setTimeout(finish, 40); // ekran kareleri işlemese de (gizli pencere) sonuç gecikmesin
  }

  function closePrompt(v: string | null) {
    app.promptReq?.resolve(v && v.trim() ? v.trim() : null);
    app.promptReq = null;
  }
  let okBtn: HTMLButtonElement | undefined = $state();
  let cancelBtn: HTMLButtonElement | undefined = $state();
  /** Pencereyi açan an Enter basılıydı (ör. Ctrl+Enter ile gönderirken): o basış
      bırakılana dek Enter onaylamaz. Yeni bir Enter basışı ise hemen onaylar. */
  let enterDown = false;
  let enterHeld = false;
  $effect(() => {
    if (app.confirmReq) enterHeld = enterDown;
  });
</script>

<!-- Outlook tarzı uyarı/soru penceresi: başlık çubuğu, solda simge, kalın soru, açıklama,
     altta gri şeritte sağa dayalı düğmeler (varsayılan düğme mavi) -->
<!-- Windows/Outlook iletişim kutusu gibi: Enter varsayılan düğme (Evet/Tamam), Esc İptal.
     Tab ile başka bir düğmeye geçildiyse Enter o düğmeye basar. Yakalama aşamasında
     işlenir: arkadaki liste/klavye kısayolları Enter'ı almaz. -->
<svelte:window onblur={() => (enterDown = enterHeld = false)} onkeyupcapture={(e) => {
  if (e.key === "Enter") {
    enterDown = false;
    enterHeld = false;
  }
}} onkeydowncapture={(e) => {
  if (e.key === "Enter" && !app.confirmReq) enterDown = true;
  if (!app.confirmReq || e.isComposing) return;
  if (e.key === "Enter") {
    const el = document.activeElement as HTMLElement | null;
    e.preventDefault();
    e.stopPropagation();
    // Basılı tutulan tuşun tekrarı (pencereyi açan basış) onaylamasın
    if (e.repeat || enterHeld) return;
    if (el?.tagName === "BUTTON" && el.closest("[data-confirm-dialog]")) el.click();
    else closeConfirm(!app.confirmReq?.safe);
  } else if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    closeConfirm(false);
  }
}} />

<AlertDialog.Root open={!!app.confirmReq} onOpenChange={(o) => !o && closeConfirm(false)}>
  <AlertDialog.Content data-confirm-dialog class="w-[min(460px,94vw)] max-w-none gap-0 overflow-hidden rounded-md p-0 data-[size=default]:sm:max-w-none" onOpenAutoFocus={(e) => { e.preventDefault(); (app.confirmReq?.safe ? cancelBtn : okBtn)?.focus(); }}>
    {@const req = app.confirmReq}
    <div class="flex h-8 items-center border-b pl-3 text-[12px]">
      <span class="text-muted-foreground flex-1">Lynora</span>
      <button class="flex h-full w-10 items-center justify-center hover:bg-[#c42b1c] hover:text-white" aria-label={t("common.close")} onclick={() => closeConfirm(false)}><X class="size-4" /></button>
    </div>
    <div class="flex gap-4 px-5 pt-5 pb-6">
      {#if req?.destructive}
        <TriangleAlert class="size-9 shrink-0 text-amber-500" strokeWidth={1.6} />
      {:else if req?.alt}
        <CircleHelp class="text-primary size-9 shrink-0" strokeWidth={1.6} />
      {:else}
        <Info class="text-primary size-9 shrink-0" strokeWidth={1.6} />
      {/if}
      <div class="min-w-0 flex-1 pt-1">
        <AlertDialog.Title class="text-[15px] leading-snug font-semibold">{req?.title}</AlertDialog.Title>
        {#if req?.text}<AlertDialog.Description class="text-muted-foreground mt-1.5 text-[13px] leading-relaxed">{req.text}</AlertDialog.Description>{/if}
      </div>
    </div>
    <div class="bg-chrome flex flex-wrap justify-end gap-2 border-t px-4 py-3">
      <button
        bind:this={okBtn}
        class="h-8 min-w-24 rounded-[4px] px-4 text-[13px] font-medium text-white outline-none focus-visible:shadow-[inset_0_0_0_1px_rgba(255,255,255,0.9),0_0_0_2px_rgba(0,0,0,0.12)] {req?.destructive ? 'bg-[#c42b1c] hover:bg-[#a52317]' : 'bg-primary hover:bg-primary/90'}"
        onclick={() => closeConfirm(true)}
      >{req?.ok ?? t("common.ok")}</button>
      {#if req?.alt}
        <button class="bg-background hover:bg-accent h-8 min-w-24 rounded-[4px] border px-4 text-[13px]" onclick={() => closeConfirm("alt")}>{req.alt}</button>
      {/if}
      <!-- Yalnızca bilgi veren uyarıda (cancel: "") tek düğme -->
      <button bind:this={cancelBtn} hidden={req?.cancel === ""} class="bg-background hover:bg-accent focus-visible:border-primary h-8 min-w-24 rounded-[4px] border px-4 text-[13px] outline-none focus-visible:shadow-[inset_0_0_0_1px_var(--primary)]" onclick={() => closeConfirm(false)}>{req?.cancel ?? t("common.cancel")}</button>
    </div>
  </AlertDialog.Content>
</AlertDialog.Root>

<Dialog.Root open={!!app.promptReq} onOpenChange={(o) => !o && closePrompt(null)}>
  <Dialog.Content class="z-[80] w-[min(460px,94vw)] max-w-none gap-0 overflow-hidden rounded-md p-0 sm:max-w-none" overlayClass="z-[80]" showCloseButton={false}>
    <div class="flex h-8 items-center border-b pl-3 text-[12px]">
      <Dialog.Title class="flex-1 text-[12px] font-normal">{app.promptReq?.title}</Dialog.Title>
      <button class="flex h-full w-10 items-center justify-center hover:bg-[#c42b1c] hover:text-white" aria-label={t("common.close")} onclick={() => closePrompt(null)}><X class="size-4" /></button>
    </div>
    <form class="grid gap-2 px-5 pt-4 pb-5" onsubmit={(e) => { e.preventDefault(); closePrompt(promptValue); }}>
      {#if app.promptReq?.label}<Label for="prompt-input" class="text-[13px] font-normal">{app.promptReq.label}</Label>{/if}
      <!-- svelte-ignore a11y_autofocus -->
      <Input id="prompt-input" class="h-8 rounded-[3px]" bind:value={promptValue} autofocus />
      <button type="submit" class="hidden">ok</button>
    </form>
    <div class="bg-chrome flex justify-end gap-2 border-t px-4 py-3">
      <button class="bg-primary hover:bg-primary/90 h-8 min-w-24 rounded-[4px] px-4 text-[13px] font-medium text-white" onclick={() => closePrompt(promptValue)}>{t("common.ok")}</button>
      <button class="bg-background hover:bg-accent h-8 min-w-24 rounded-[4px] border px-4 text-[13px]" onclick={() => closePrompt(null)}>{t("common.cancel")}</button>
    </div>
  </Dialog.Content>
</Dialog.Root>
