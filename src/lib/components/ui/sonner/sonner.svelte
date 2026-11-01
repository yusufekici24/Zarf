<script lang="ts">
	import { mode } from "mode-watcher";
	import { Toaster as Sonner, type ToasterProps as SonnerProps } from "svelte-sonner";
	import Loader2Icon from "@lucide/svelte/icons/loader-circle";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import OctagonXIcon from "@lucide/svelte/icons/octagon-x";
	import InfoIcon from "@lucide/svelte/icons/info";
	import TriangleAlertIcon from "@lucide/svelte/icons/triangle-alert";

	let { ...restProps }: SonnerProps = $props();
</script>

<Sonner
	theme={mode.current}
	class="toaster group"
	style="--normal-bg: var(--color-popover); --normal-text: var(--color-popover-foreground); --normal-border: var(--color-border); --width: 380px;"
	visibleToasts={4}
	toastOptions={{
		// Yeni posta uyarı kartıyla aynı tasarım: 4px köşeli yarı saydam kart, ince kenar,
		// yumuşak gölge, solda yuvarlak renkli simge, kalın başlık, gri açıklama, içte kapat
		classes: {
			toast: "!rounded-[4px] !border !border-black/10 dark:!border-white/15 !bg-popover !py-3 !pr-9 !pl-3.5 !gap-3 !items-start !text-[13px] !shadow-[0_8px_28px_rgba(0,0,0,0.18)] [&_[data-icon]]:!m-0 [&_[data-icon]]:!flex [&_[data-icon]]:!size-9 [&_[data-icon]]:!shrink-0 [&_[data-icon]]:!items-center [&_[data-icon]]:!justify-center [&_[data-icon]]:!rounded-full [&_[data-icon]]:!bg-primary/10 [&_[data-icon]]:!text-primary",
			success: "[&_[data-icon]]:!bg-emerald-600/10 [&_[data-icon]]:!text-emerald-600",
			error: "[&_[data-icon]]:!bg-red-600/10 [&_[data-icon]]:!text-red-600",
			warning: "[&_[data-icon]]:!bg-amber-500/15 [&_[data-icon]]:!text-amber-600",
			info: "",
			title: "!text-[14px] !font-semibold !leading-5",
			description: "!text-[12.5px] !text-muted-foreground !leading-[18px]",
			actionButton: "!bg-primary !text-primary-foreground !h-8 !rounded-[4px] !px-3 !text-[12px] !font-medium !self-center",
			cancelButton: "!h-8 !rounded-[4px] !px-3 !text-[12px] !self-center",
			closeButton: "!left-auto !right-1.5 !top-1.5 !transform-none !size-6 !rounded-[4px] !border-0 !bg-transparent !text-muted-foreground hover:!bg-accent hover:!text-foreground",
		},
	}}
	{...restProps}
>
	{#snippet loadingIcon()}
		<Loader2Icon class="size-[18px] animate-spin" />
	{/snippet}
	{#snippet successIcon()}
		<CircleCheckIcon class="size-[18px]" />
	{/snippet}
	{#snippet errorIcon()}
		<OctagonXIcon class="size-[18px]" />
	{/snippet}
	{#snippet infoIcon()}
		<InfoIcon class="size-[18px]" />
	{/snippet}
	{#snippet warningIcon()}
		<TriangleAlertIcon class="size-[18px]" />
	{/snippet}
</Sonner>
