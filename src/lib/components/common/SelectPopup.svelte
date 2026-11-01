<script lang="ts">
  // Uygulamadaki tüm <select> kutularının açılır listesi: işletim sisteminin (GTK) listesi
  // yerine uygulamanın menü görünümüyle açılır. Seçim kutunun kendisine yazılır ve
  // "input"/"change" olayları gönderilir, böylece mevcut bağlamalar aynen çalışır.
  // Liste odak almaz (açık iletişim kutuları/paneller kapanmasın diye); klavye kutudan yönetilir.
  import { onMount, tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";

  type Opt = { value: string; label: string; disabled: boolean; group: string };
  let sel = $state<HTMLSelectElement | null>(null);
  let opts = $state<Opt[]>([]);
  let active = $state(-1);
  let pos = $state({ left: 0, top: 0, width: 0, maxH: 320, up: false, bottom: 0 });
  let list: HTMLDivElement | undefined = $state();
  let typed = "";
  let typedAt = 0;

  function usable(t: EventTarget | null): t is HTMLSelectElement {
    return t instanceof HTMLSelectElement && !t.multiple && t.size <= 1 && !t.disabled;
  }

  async function open(s: HTMLSelectElement) {
    sel = s;
    opts = Array.from(s.options).map((o) => ({ value: o.value, label: o.label || o.text, disabled: o.disabled, group: o.parentElement instanceof HTMLOptGroupElement ? o.parentElement.label : "" }));
    active = s.selectedIndex;
    const r = s.getBoundingClientRect();
    const below = window.innerHeight - r.bottom - 8;
    const above = r.top - 8;
    const want = Math.min(320, opts.length * 30 + 8);
    const up = below < want && above > below;
    pos = { left: r.left, top: r.bottom + 2, width: r.width, maxH: Math.max(120, Math.min(320, up ? above : below)), up, bottom: window.innerHeight - r.top + 2 };
    await tick();
    list?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
    // Pencereden taşmasın
    if (list) {
      const lr = list.getBoundingClientRect();
      if (lr.right > window.innerWidth - 4) pos.left = Math.max(4, window.innerWidth - 4 - lr.width);
    }
  }
  function close() {
    sel = null;
    opts = [];
  }
  function choose(i: number) {
    const s = sel;
    const o = opts[i];
    if (!s || !o || o.disabled) return;
    close();
    if (s.value !== o.value) {
      s.value = o.value;
      s.dispatchEvent(new Event("input", { bubbles: true }));
      s.dispatchEvent(new Event("change", { bubbles: true }));
    }
    s.focus();
  }
  function step(d: number) {
    let i = active;
    for (let n = 0; n < opts.length; n++) {
      i = (i + d + opts.length) % opts.length;
      if (!opts[i].disabled) break;
    }
    active = i;
    void tick().then(() => list?.querySelector<HTMLElement>(`[data-i="${i}"]`)?.scrollIntoView({ block: "nearest" }));
  }

  onMount(() => {
    const down = (e: MouseEvent) => {
      if (sel && list?.contains(e.target as Node)) return;
      if (e.button === 0 && usable(e.target)) {
        e.preventDefault(); // işletim sisteminin listesi açılmasın
        const s = e.target;
        if (sel === s) close();
        else {
          s.focus();
          void open(s);
        }
        return;
      }
      if (sel) close();
    };
    const key = (e: KeyboardEvent) => {
      const t = e.target;
      if (!sel) {
        // Alt+↓, F4, boşluk: kutu klavyeden açılır
        if (usable(t) && ((e.altKey && (e.key === "ArrowDown" || e.key === "ArrowUp")) || e.key === "F4" || e.key === " ")) {
          e.preventDefault();
          void open(t);
        }
        return;
      }
      if (e.key === "Escape" || e.key === "Tab") {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
        }
        close();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        step(e.key === "ArrowDown" ? 1 : -1);
      } else if (e.key === "Home" || e.key === "End") {
        e.preventDefault();
        active = e.key === "Home" ? -1 : opts.length;
        step(e.key === "Home" ? 1 : -1);
      } else if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        e.stopPropagation();
        choose(active);
      } else if (e.key.length === 1) {
        // Yazarak ara
        const now = Date.now();
        typed = now - typedAt > 800 ? e.key : typed + e.key;
        typedAt = now;
        const q = typed.toLocaleLowerCase();
        const i = opts.findIndex((o) => !o.disabled && o.label.toLocaleLowerCase().startsWith(q));
        if (i >= 0) {
          active = i;
          void tick().then(() => list?.querySelector<HTMLElement>(`[data-i="${i}"]`)?.scrollIntoView({ block: "nearest" }));
        }
      }
    };
    const away = () => sel && close();
    document.addEventListener("mousedown", down, true);
    document.addEventListener("keydown", key, true);
    window.addEventListener("resize", away);
    window.addEventListener("blur", away);
    return () => {
      document.removeEventListener("mousedown", down, true);
      document.removeEventListener("keydown", key, true);
      window.removeEventListener("resize", away);
      window.removeEventListener("blur", away);
    };
  });
</script>

{#if sel}
  <!-- Dışarı tıklamayı algılayan açık paneller/pencereler bu listeye yapılan tıklamayı
       dışarı tıklama saymasın diye olayın yayılması burada durdurulur -->
  <div
    bind:this={list}
    data-select-popup
    role="listbox"
    tabindex="-1"
    class="bg-popover text-popover-foreground fixed z-[400] overflow-y-auto overscroll-contain rounded-md border p-1 text-[13px] shadow-md"
    style="pointer-events:auto;left:{pos.left}px;min-width:{pos.width}px;max-width:min(560px, calc(100vw - 8px));max-height:{pos.maxH}px;{pos.up ? `bottom:${pos.bottom}px` : `top:${pos.top}px`}"
    onpointerdown={(e) => e.stopPropagation()}
    onmousedown={(e) => {
      e.preventDefault();
      e.stopPropagation();
    }}
  >
    {#each opts as o, i (i)}
      {#if o.group && opts[i - 1]?.group !== o.group}
        <div class="text-muted-foreground px-2 pt-1.5 pb-1 text-[11px] font-semibold">{o.group}</div>
      {/if}
      <div
        data-i={i}
        role="option"
        aria-selected={sel?.selectedIndex === i}
        aria-disabled={o.disabled}
        class="relative flex min-h-7 cursor-default items-center rounded-sm py-1 pr-2 pl-7 whitespace-nowrap select-none {o.disabled ? 'opacity-40' : i === active ? 'bg-accent text-accent-foreground' : ''}"
        onmouseenter={() => !o.disabled && (active = i)}
        onclick={() => choose(i)}
        onkeydown={() => {}}
      >
        {#if sel?.selectedIndex === i}<Check class="absolute left-2 size-4" />{/if}
        {o.label}
      </div>
    {/each}
  </div>
{/if}
