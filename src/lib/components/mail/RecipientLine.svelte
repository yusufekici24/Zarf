<script lang="ts">
  // Okuma bölmesinde Kime/Bilgi (Outlook gibi): tek satıra sığan kişiler ve "+N diğer";
  // tıklayınca tamamı aşağı doğru açılır (uzunsa kaydırılır). Dağıtım listelerinin
  // solunda "+" vardır: tıklayınca grubun üyeleri satırda gösterilir.
  import { displayName, splitAddresses, splitNameAddr } from "$lib/format";
  import { t } from "$lib/i18n.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/stores/app.svelte";
  import { knownGroups } from "$lib/components/common/AddressInput.svelte";
  import Plus from "@lucide/svelte/icons/plus";
  import Minus from "@lucide/svelte/icons/minus";
  import { toast } from "svelte-sonner";
  import { people } from "$lib/stores/people.svelte";
  import Avatar from "$lib/components/common/Avatar.svelte";

  /** Adı yoksa dizindeki görünen ad (Outlook gibi "BS Çevresel Sistemler"), yoksa adres */
  function nameOf(entry: string): string {
    const { name, addr } = splitNameAddr(entry);
    return (name && !name.includes("@") ? name : "") || (addr && people.name(addr)) || addr || entry;
  }

  let { label, value }: { label: string; value: string } = $props();

  let expanded = $state(false);
  /** Açılmış gruplar: adres → üyeler */
  let members = $state<Record<string, string[]>>({});
  let busy = $state("");

  type Entry = { text: string; addr: string; depth: number; group: boolean };
  const list = $derived.by<Entry[]>(() => {
    const out: Entry[] = [];
    for (const a of splitAddresses(value)) {
      const addr = (splitNameAddr(a).addr || a).toLowerCase();
      const group = knownGroups.has(addr);
      out.push({ text: a, addr, depth: 0, group });
      if (group && members[addr]) for (const m of members[addr]) out.push({ text: m, addr: (splitNameAddr(m).addr || m).toLowerCase(), depth: 1, group: false });
    }
    return out;
  });

  // Gruplar (bir kez) sunucuya sorulur
  $effect(() => {
    const addrs = splitAddresses(value).map((a) => (splitNameAddr(a).addr || a).toLowerCase()).filter((a) => a.includes("@"));
    if (!addrs.length || !app.accounts.some((a) => a.kind === "exchange")) return;
    void api.groupAddresses(addrs).then((g) => g.forEach((x) => knownGroups.add(x.toLowerCase()))).catch(() => {});
  });

  async function toggleGroup(addr: string) {
    if (members[addr]) {
      const { [addr]: _, ...rest } = members;
      members = rest;
      return;
    }
    busy = addr;
    try {
      members = { ...members, [addr]: await api.expandGroup(addr) };
      expanded = true;
    } catch (e) {
      toast.error(String(e));
    } finally {
      busy = "";
    }
  }

  // Tek satıra kaç kişi sığıyor? (Genişlik değişince yeniden ölçülür)
  let box: HTMLDivElement | undefined = $state();
  let fit = $state(Infinity);
  function measure() {
    if (!box || expanded || !box.clientWidth) return;
    const items = Array.from(box.querySelectorAll<HTMLElement>("[data-r]"));
    // "+N diğer" için yer bırakılır
    const limit = box.clientWidth - (items.length > 1 ? 78 : 0);
    let n = 0;
    for (const el of items) {
      if (el.offsetLeft + el.offsetWidth > limit && n > 0) break;
      n++;
    }
    // Hepsi sığıyorsa düğmeye yer ayırmaya gerek yok
    const last = items[items.length - 1];
    fit = last && last.offsetLeft + last.offsetWidth <= box.clientWidth ? items.length : Math.max(1, n);
  }
  $effect(() => {
    void list;
    void expanded;
    if (!box) return;
    const ro = new ResizeObserver(() => measure());
    ro.observe(box);
    // İlk çizimden önce ölçülür ("+N diğer" sonradan belirmesin). Gizli (hazırda bekleyen)
    // pencerede requestAnimationFrame çalışmadığı için doğrudan ve zamanlayıcıyla da ölçülür.
    measure();
    const tm = setTimeout(measure, 0);
    return () => {
      ro.disconnect();
      clearTimeout(tm);
    };
  });

  const hidden = $derived(expanded ? 0 : Math.max(0, list.length - fit));
</script>

<div class="text-muted-foreground flex min-w-0 items-start gap-1 text-xs leading-5">
  <span class="shrink-0">{label}</span>
  <div
    bind:this={box}
    class="selectable relative min-w-0 flex-1 {expanded ? 'max-h-24 overflow-y-auto overscroll-contain pr-1' : 'flex overflow-hidden whitespace-nowrap'}"
  >
    {#each list as r, i (i + r.text)}
      <span data-r class="inline-flex h-5 max-w-full shrink-0 items-center align-top leading-5 whitespace-nowrap {r.depth ? 'pl-3' : ''}" style={!expanded && i >= fit ? "visibility:hidden" : ""}>
        {#if r.group}
          <button
            class="text-primary hover:bg-primary/15 mr-0.5 rounded-sm p-px"
            title={t("compose.expand_group")}
            disabled={busy === r.addr}
            onclick={() => toggleGroup(r.addr)}
          >
            {#if members[r.addr]}<Minus class="size-3" />{:else}<Plus class="size-3 {busy === r.addr ? 'animate-spin' : ''}" />{/if}
          </button>
        {:else}
          <!-- Kişinin fotoğrafı (yoksa baş harfleri), yeni iletideki gibi -->
          <Avatar name={nameOf(r.text)} email={r.addr} class="mr-1 size-4 self-center text-[7px] leading-none" />
        {/if}
        <button
          data-card-trigger
          class="text-foreground hover:text-primary min-w-0 truncate font-semibold hover:underline"
          title={splitNameAddr(r.text).addr || r.text}
          onclick={(e) => people.openCard(r.text, e)}
        >{nameOf(r.text)}</button>{#if i < list.length - 1}<span class="pr-1">;</span>{/if}
      </span>
    {/each}
    {#if expanded && list.length > 1}
      <button class="text-primary ml-1 hover:underline" onclick={() => (expanded = false)}>{t("mail.show_less")}</button>
    {/if}
  </div>
  {#if hidden > 0}
    <button class="text-primary shrink-0 font-semibold hover:underline" onclick={() => (expanded = true)}>+{hidden} {t("mail.n_others")}</button>
  {/if}
</div>
