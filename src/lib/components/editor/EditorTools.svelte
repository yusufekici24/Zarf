<script lang="ts">
  // Düzenleyicinin biçim araçları, tek yerde: ileti penceresinin şeridi (basic / font / paragraph),
  // düzenleyicinin kendi araç çubuğu (bar) ve seçim araç çubuğu (mini) aynı düğmeleri kullanır.
  // Düğmeler odağı almaz (mousedown engellenir), açık olan biçim vurgulu görünür.
  import type { Component } from "svelte";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
  import ColorButton from "./ColorButton.svelte";
  import { COMMON_FONTS, FONT_SIZES, fontStack } from "$lib/fonts";
  import type { CaretState, CaseMode, EditorApi } from "$lib/editor";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import Bold from "@lucide/svelte/icons/bold";
  import Italic from "@lucide/svelte/icons/italic";
  import Underline from "@lucide/svelte/icons/underline";
  import Strikethrough from "@lucide/svelte/icons/strikethrough";
  import Subscript from "@lucide/svelte/icons/subscript";
  import Superscript from "@lucide/svelte/icons/superscript";
  import List from "@lucide/svelte/icons/list";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import Indent from "@lucide/svelte/icons/indent-increase";
  import Outdent from "@lucide/svelte/icons/indent-decrease";
  import AlignLeft from "@lucide/svelte/icons/align-left";
  import AlignCenter from "@lucide/svelte/icons/align-center";
  import AlignRight from "@lucide/svelte/icons/align-right";
  import AlignJustify from "@lucide/svelte/icons/align-justify";
  import Eraser from "@lucide/svelte/icons/eraser";
  import Highlighter from "@lucide/svelte/icons/highlighter";
  import Baseline from "@lucide/svelte/icons/baseline";
  import Grow from "@lucide/svelte/icons/a-arrow-up";
  import Shrink from "@lucide/svelte/icons/a-arrow-down";
  import CaseSensitive from "@lucide/svelte/icons/case-sensitive";
  import LineHeight from "@lucide/svelte/icons/list-chevrons-up-down";
  import LinkIcon from "@lucide/svelte/icons/link";
  import Paintbrush from "@lucide/svelte/icons/paintbrush";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

  let {
    editor,
    caret,
    layout,
  }: {
    editor: EditorApi | undefined;
    caret: CaretState;
    layout: "basic" | "font" | "paragraph" | "bar" | "mini";
  } = $props();

  type Icon = Component<{ class?: string }>;
  type Toggle = [Icon, string, string, boolean];
  const STYLE = $derived<Toggle[]>([[Bold, "bold", "editor.bold", caret.bold], [Italic, "italic", "editor.italic", caret.italic], [Underline, "underline", "editor.underline", caret.underline], [Strikethrough, "strikeThrough", "editor.strike", caret.strike]]);
  const SCRIPT = $derived<Toggle[]>([[Subscript, "subscript", "editor.subscript", caret.sub], [Superscript, "superscript", "editor.superscript", caret.sup]]);
  const LISTS = $derived<Toggle[]>([[List, "insertUnorderedList", "editor.bullets", caret.ul], [ListOrdered, "insertOrderedList", "editor.numbering", caret.ol]]);
  const DENT: Toggle[] = [[Outdent, "outdent", "editor.outdent", false], [Indent, "indent", "editor.indent", false]];
  const ALIGN = $derived<Toggle[]>([[AlignLeft, "justifyLeft", "editor.left", caret.align === "left"], [AlignCenter, "justifyCenter", "editor.center", caret.align === "center"], [AlignRight, "justifyRight", "editor.right", caret.align === "right"], [AlignJustify, "justifyFull", "editor.justify", caret.align === "justify"]]);
  const CASES: [CaseMode, string][] = [["sentence", "editor.case_sentence"], ["lower", "editor.case_lower"], ["upper", "editor.case_upper"], ["title", "editor.case_title"]];
  const SPACING = ["1", "1.15", "1.5", "2", "2.5", "3"];

  // Bu bilgisayardaki yazı tipleri bir kez sorulur (bütün araç çubukları paylaşır)
  let systemFonts = $state<string[]>([]);
  $effect(() => {
    if (layout === "mini" || layout === "paragraph") return;
    fontList().then((f) => (systemFonts = f));
  });
  const hold = (e: Event) => e.preventDefault();
</script>

<script lang="ts" module>
  let fontsOnce: Promise<string[]> | undefined;
  function fontList(): Promise<string[]> {
    fontsOnce ??= api.systemFonts().then((f) => (f ?? []).filter((x) => !COMMON_FONTS.includes(x))).catch(() => []);
    return fontsOnce;
  }
</script>

{#snippet toggles(list: Toggle[])}
  {#each list as [Icon, cmd, label, on] (cmd)}
    <button type="button" class="ed-btn" data-cmd={cmd} data-active={on} aria-pressed={on} title={t(label)} aria-label={t(label)} onmousedown={hold} onclick={() => editor?.exec(cmd)}><Icon class="size-4" /></button>
  {/each}
{/snippet}

{#snippet button(Icon: Icon, label: string, run: () => void, name: string, on = false)}
  <button type="button" class="ed-btn" data-cmd={name} data-active={on} title={t(label)} aria-label={t(label)} onmousedown={hold} onclick={run}><Icon class="size-4" /></button>
{/snippet}

{#snippet sep()}<span class="bg-border mx-0.5 h-4 w-px shrink-0"></span>{/snippet}

{#snippet family(width: string)}
  <select class="h-6 {width} rounded border bg-transparent px-1 text-xs" title={t("editor.font")} aria-label={t("editor.font")} value={caret.family} onchange={(e) => editor?.exec("fontName", (e.target as HTMLSelectElement).value)}>
    {#if caret.family && !COMMON_FONTS.includes(caret.family) && !systemFonts.includes(caret.family)}<option value={caret.family}>{caret.family}</option>{/if}
    <optgroup label={t("compose.fonts_common")}>
      {#each COMMON_FONTS as f (f)}<option value={f} style="font-family:{fontStack(f)}">{f}</option>{/each}
    </optgroup>
    {#if systemFonts.length}
      <optgroup label={t("compose.fonts_system")}>
        {#each systemFonts as f (f)}<option value={f} style="font-family:{fontStack(f)}">{f}</option>{/each}
      </optgroup>
    {/if}
  </select>
{/snippet}

{#snippet size()}
  <select class="h-6 w-14 rounded border bg-transparent px-1 text-xs" title={t("editor.size")} aria-label={t("editor.size")} value={String(caret.size)} onchange={(e) => editor?.setFontSize(Number((e.target as HTMLSelectElement).value))}>
    {#if caret.size && !FONT_SIZES.includes(caret.size)}<option value={String(caret.size)}>{caret.size}</option>{/if}
    {#each FONT_SIZES as v (v)}<option value={String(v)}>{v}</option>{/each}
  </select>
{/snippet}

{#snippet steps()}
  {@render button(Grow, "editor.grow", () => editor?.stepFontSize(1), "grow")}
  {@render button(Shrink, "editor.shrink", () => editor?.stepFontSize(-1), "shrink")}
{/snippet}

{#snippet colors()}
  <ColorButton icon={Highlighter} label={t("editor.highlight")} kind="back" onpick={(c) => editor?.setColor("back", c)} />
  <ColorButton icon={Baseline} label={t("editor.color")} kind="fore" onpick={(c) => editor?.setColor("fore", c)} />
{/snippet}

{#snippet clear()}{@render button(Eraser, "editor.clear_format", () => editor?.exec("removeFormat"), "removeFormat")}{/snippet}

{#snippet menu(Icon: Icon, label: string, name: string, items: [string, string][], pick: (v: string) => void)}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <button {...props} type="button" class="ed-btn" data-cmd={name} title={t(label)} aria-label={t(label)} onmousedown={hold}><Icon class="size-4" /><ChevronDown class="size-3 opacity-70" /></button>
      {/snippet}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content class="w-auto min-w-36" onCloseAutoFocus={hold}>
      {#each items as [value, text] (value)}<DropdownMenu.Item onclick={() => pick(value)}>{text}</DropdownMenu.Item>{/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/snippet}

{#snippet spacing()}
  {@render menu(LineHeight, "editor.line_spacing", "lineHeight", SPACING.map((v): [string, string] => [v, Number(v).toLocaleString(undefined, { minimumFractionDigits: 1 })]), (v) => editor?.setLineHeight(v))}
{/snippet}

{#snippet cases()}
  {@render menu(CaseSensitive, "editor.change_case", "case", CASES.map(([m, l]): [string, string] => [m, t(l)]), (v) => editor?.changeCase(v as CaseMode))}
{/snippet}

{#if layout === "basic"}
  <div class="flex flex-col gap-1 py-0.5">
    <div class="flex items-center gap-0.5">
      {@render family("w-36")}{@render size()}{@render steps()}{@render sep()}{@render toggles(LISTS)}{@render toggles(DENT)}{@render sep()}{@render clear()}
    </div>
    <div class="flex items-center gap-0.5">
      {@render toggles(STYLE)}{@render colors()}{@render sep()}{@render toggles(ALIGN)}{@render sep()}{@render spacing()}
    </div>
  </div>
{:else if layout === "font"}
  <div class="flex flex-col gap-1 py-0.5">
    <div class="flex items-center gap-0.5">{@render family("w-40")}{@render size()}{@render steps()}{@render cases()}{@render clear()}</div>
    <div class="flex items-center gap-0.5">{@render toggles(STYLE)}{@render toggles(SCRIPT)}{@render sep()}{@render colors()}</div>
  </div>
{:else if layout === "paragraph"}
  <div class="flex flex-col gap-1 py-0.5">
    <div class="flex items-center gap-0.5">{@render toggles(LISTS)}{@render toggles(DENT)}</div>
    <div class="flex items-center gap-0.5">{@render toggles(ALIGN)}{@render spacing()}</div>
  </div>
{:else if layout === "bar"}
  {@render family("w-32")}{@render size()}{@render sep()}{@render toggles(STYLE)}{@render colors()}{@render sep()}{@render toggles(LISTS)}{@render toggles(DENT)}{@render toggles(ALIGN)}{@render spacing()}{@render sep()}{@render clear()}
{:else}
  {@render steps()}{@render sep()}{@render toggles(STYLE.slice(0, 3))}{@render colors()}{@render sep()}{@render toggles(LISTS)}{@render sep()}
  {@render button(LinkIcon, "editor.insert_link", () => void editor?.link(), "link", caret.link)}
  {@render button(Paintbrush, "editor.painter", () => editor?.painter(), "painter", caret.painter > 0)}
  {@render clear()}
{/if}
