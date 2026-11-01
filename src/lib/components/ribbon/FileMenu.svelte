<script lang="ts">
  // Şeritteki "Dosya" sekmesi: Outlook gibi tam pencere Dosya görünümünü açar
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n.svelte";
</script>

<button class="rounded-t px-2.5 pt-1 pb-1.5 hover:bg-black/5 dark:hover:bg-white/10" onclick={() => (app.backstage = true)}>{t("menu.file")}</button>
