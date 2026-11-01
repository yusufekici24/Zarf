<script lang="ts">
	import { Dialog as DialogPrimitive } from "bits-ui";
	import XIcon from "@lucide/svelte/icons/x";
	import { Button } from "$lib/components/ui/button/index.js";
	import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";
	import * as Dialog from "./index.js";
	import DialogPortal from "./dialog-portal.svelte";
	import type { Snippet } from "svelte";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		class: className,
		portalProps,
		overlayClass = "",
		children,
		showCloseButton = true,
		noOverlay = false,
		...restProps
	}: WithoutChildrenOrChild<DialogPrimitive.ContentProps> & {
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof DialogPortal>>;
		/** Üst üste açılan pencereler için katman sınıfı (ör. z-[80]) */
		overlayClass?: string;
		children: Snippet;
		showCloseButton?: boolean;
		/** Kipsiz pencere: arka plan kullanılabilir kalır (ör. ileti yazma) */
		noOverlay?: boolean;
	} = $props();

	/**
	 * Windows iletişim kutusu gibi: tek satırlı bir alandayken Enter varsayılan düğmeyi
	 * (data-default-button) çalıştırır. Alanın kendi Enter işi varsa (öneri listesi vb.)
	 * o önce gelir (defaultPrevented).
	 */
	function enterDefault(e: KeyboardEvent) {
		if (e.key !== "Enter" || e.defaultPrevented || e.isComposing || e.shiftKey || e.ctrlKey || e.altKey) return;
		const el = e.target as HTMLInputElement;
		if (el?.tagName !== "INPUT" || ["checkbox", "radio", "button", "submit", "file", "color", "range"].includes(el.type)) return;
		const box = e.currentTarget as HTMLElement;
		const btn = [...box.querySelectorAll<HTMLButtonElement>("[data-default-button]")].find((b) => b.closest("[data-slot=dialog-content]") === box.closest("[data-slot=dialog-content]") && !b.disabled);
		if (!btn) return;
		e.preventDefault();
		btn.click();
	}
</script>

<DialogPortal {...portalProps}>
	{#if !noOverlay}<Dialog.Overlay class={overlayClass} />{/if}
	<!-- Ortalama dönüşümsüz (translate yazıyı bulanıklaştırır) ve tüm WebKit sürümlerinde
	     içerik yüksekliğinde: tam ekran esnek kapsayıcı içinde kart -->
	<DialogPrimitive.Content bind:ref data-slot="dialog-content" {...restProps}>
		{#snippet child({ props })}
			<div class={cn("pointer-events-none fixed inset-0 z-50 flex items-center justify-center p-4", overlayClass)}>
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div
					{...props}
					onkeydown={(e) => { (props.onkeydown as ((e: KeyboardEvent) => void) | undefined)?.(e); enterDefault(e); }}
					class={cn(
						"bg-popover text-popover-foreground data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 ring-foreground/10 pointer-events-auto relative flex h-auto max-h-full w-full flex-col self-center max-w-[calc(100%-2rem)] gap-6 rounded-xl p-6 text-sm ring-1 shadow-xl duration-100 sm:max-w-md outline-none",
						className
					)}
				>
					{@render children?.()}
					{#if showCloseButton}
						<DialogPrimitive.Close data-slot="dialog-close">
							{#snippet child({ props: closeProps })}
								<Button variant="ghost" class="absolute top-4 right-4" size="icon-sm" {...closeProps}>
									<XIcon />
									<span class="sr-only">Close</span>
								</Button>
							{/snippet}
						</DialogPrimitive.Close>
					{/if}
				</div>
			</div>
		{/snippet}
	</DialogPrimitive.Content>
</DialogPortal>
