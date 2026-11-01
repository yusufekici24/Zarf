<script lang="ts">
  // Kişi resmi: Exchange/AD fotoğrafı varsa o, yoksa renkli baş harfler (Outlook gibi).
  import { avatarColor, initials } from "$lib/format";
  import { photos } from "$lib/stores/photos.svelte";

  let { name, email = "", class: cls = "size-8 text-xs" }: { name: string; email?: string; class?: string } = $props();

  const url = $derived(email ? photos.get(email) : null);
  let broken = $state(false);
</script>

{#if url && !broken}
  <img src={url} alt="" class="shrink-0 rounded-full object-cover {cls}" onerror={() => (broken = true)} />
{:else}
  <div class="flex shrink-0 items-center justify-center rounded-full font-semibold text-white {cls}" style="background:{avatarColor(name || email)}">{initials(name || email)}</div>
{/if}
