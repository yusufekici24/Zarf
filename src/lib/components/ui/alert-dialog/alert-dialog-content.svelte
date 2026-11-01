<script lang="ts">
	import { AlertDialog as AlertDialogPrimitive } from "bits-ui";
	import { cn, type WithoutChild, type WithoutChildrenOrChild } from "$lib/utils.js";
	import AlertDialogOverlay from "./alert-dialog-overlay.svelte";
	import AlertDialogPortal from "./alert-dialog-portal.svelte";
	import type { ComponentProps } from "svelte";

	let {
		ref = $bindable(null),
		class: className,
		size = "default",
		portalProps,
		children,
		...restProps
	}: WithoutChild<AlertDialogPrimitive.ContentProps> & {
		size?: "default" | "sm";
		portalProps?: WithoutChildrenOrChild<ComponentProps<typeof AlertDialogPortal>>;
	} = $props();
</script>

<AlertDialogPortal {...portalProps}>
	<AlertDialogOverlay />
	<!-- Ortalama dönüşümsüz (translate yazıyı bulanıklaştırır) ve tüm WebKit sürümlerinde
	     içerik yüksekliğinde: tam ekran esnek kapsayıcı içinde kart -->
	<AlertDialogPrimitive.Content bind:ref data-slot="alert-dialog-content" data-size={size} {...restProps}>
		{#snippet child({ props })}
			<div class="pointer-events-none fixed inset-0 z-[80] flex items-center justify-center p-4">
				<div
					{...props}
					class={cn(
						"data-open:animate-in data-closed:animate-out data-closed:fade-out-0 data-open:fade-in-0 bg-popover text-popover-foreground ring-foreground/10 pointer-events-auto h-auto self-center gap-6 rounded-xl p-6 ring-1 shadow-xl duration-100 data-[size=default]:max-w-xs data-[size=sm]:max-w-xs data-[size=default]:sm:max-w-lg group/alert-dialog-content relative flex max-h-full w-full flex-col outline-none",
						className
					)}
				>
					{@render children?.()}
				</div>
			</div>
		{/snippet}
	</AlertDialogPrimitive.Content>
</AlertDialogPortal>
