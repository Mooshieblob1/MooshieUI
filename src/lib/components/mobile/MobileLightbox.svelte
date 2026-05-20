<script lang="ts">
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import type { OutputImage } from "../../types/index.js";
  import {
    sendImageToImg2Img,
    sendImageToUpscale,
  } from "../../utils/galleryActions.js";
  import MobileActionSheet from "./MobileActionSheet.svelte";
  import type { ActionSheetItem } from "./MobileActionSheet.svelte";

  interface Props {
    images: OutputImage[];
    onSwitchToGenerate?: () => void;
  }
  let { images, onSwitchToGenerate }: Props = $props();

  let actionOpen = $state(false);

  function tt(key: string, fb: string) {
    const v = locale.t(key);
    return v === key ? fb : v;
  }

  // Touch / pinch / pan state
  let imgEl = $state<HTMLImageElement | null>(null);
  let scale = $state(1);
  let translateX = $state(0);
  let translateY = $state(0);
  let pinchStartDist = 0;
  let pinchStartScale = 1;
  let panStartX = 0;
  let panStartY = 0;
  let panBaseX = 0;
  let panBaseY = 0;
  let panActive = $state(false);
  let swipeStartX = 0;
  let swipeStartY = 0;
  let swipeActive = $state(false);
  let swipeDX = $state(0);

  const currentIndex = $derived.by(() => {
    if (!gallery.selectedImage) return -1;
    return images.findIndex(
      (i) =>
        i.filename === gallery.selectedImage!.filename &&
        i.subfolder === gallery.selectedImage!.subfolder,
    );
  });

  function navigate(dir: -1 | 1) {
    if (currentIndex < 0) return;
    const next = currentIndex + dir;
    if (next < 0 || next >= images.length) return;
    resetTransform();
    gallery.openLightbox(images[next]);
  }

  function resetTransform() {
    scale = 1;
    translateX = 0;
    translateY = 0;
    swipeDX = 0;
  }

  function close() {
    resetTransform();
    gallery.closeLightbox();
  }

  function applyTransform() {
    if (!imgEl) return;
    imgEl.style.transform = `translate(${translateX + swipeDX}px, ${translateY}px) scale(${scale})`;
  }

  $effect(() => {
    // re-apply on state change
    if (imgEl) applyTransform();
  });

  // Active touches map for pinch
  const activeTouches = new Map<number, { x: number; y: number }>();

  function dist(a: { x: number; y: number }, b: { x: number; y: number }) {
    const dx = a.x - b.x;
    const dy = a.y - b.y;
    return Math.hypot(dx, dy);
  }

  function onPointerDown(e: PointerEvent) {
    activeTouches.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (activeTouches.size === 1) {
      if (scale > 1.01) {
        // Pan an already-zoomed image
        panActive = true;
        panStartX = e.clientX;
        panStartY = e.clientY;
        panBaseX = translateX;
        panBaseY = translateY;
      } else {
        // Track horizontal swipe for nav
        swipeActive = true;
        swipeStartX = e.clientX;
        swipeStartY = e.clientY;
      }
    } else if (activeTouches.size === 2) {
      panActive = false;
      swipeActive = false;
      swipeDX = 0;
      const pts = [...activeTouches.values()];
      pinchStartDist = dist(pts[0], pts[1]);
      pinchStartScale = scale;
    }
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!activeTouches.has(e.pointerId)) return;
    activeTouches.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (activeTouches.size >= 2) {
      const pts = [...activeTouches.values()];
      const d = dist(pts[0], pts[1]);
      if (pinchStartDist > 0) {
        scale = Math.max(1, Math.min(6, pinchStartScale * (d / pinchStartDist)));
        applyTransform();
      }
    } else if (panActive) {
      translateX = panBaseX + (e.clientX - panStartX);
      translateY = panBaseY + (e.clientY - panStartY);
      applyTransform();
    } else if (swipeActive) {
      const dx = e.clientX - swipeStartX;
      const dy = e.clientY - swipeStartY;
      // Only treat as horizontal swipe if dominantly horizontal
      if (Math.abs(dx) > Math.abs(dy)) {
        swipeDX = dx;
        applyTransform();
      }
    }
  }

  function onPointerUp(e: PointerEvent) {
    activeTouches.delete(e.pointerId);
    if (activeTouches.size < 2) pinchStartDist = 0;
    if (panActive && activeTouches.size === 0) {
      panActive = false;
    }
    if (swipeActive && activeTouches.size === 0) {
      swipeActive = false;
      const threshold = (window.innerWidth || 360) * 0.25;
      if (swipeDX > threshold) {
        swipeDX = 0;
        navigate(-1);
        return;
      } else if (swipeDX < -threshold) {
        swipeDX = 0;
        navigate(1);
        return;
      }
      swipeDX = 0;
      applyTransform();
    }
    if (scale < 1.05) {
      scale = 1;
      translateX = 0;
      translateY = 0;
      applyTransform();
    }
  }

  function onDoubleClick() {
    if (scale > 1.01) {
      resetTransform();
    } else {
      scale = 2.5;
    }
    applyTransform();
  }

  // Reset transform when image changes
  $effect(() => {
    if (gallery.lightboxUrl) {
      resetTransform();
    }
  });

  // Keyboard nav
  function onKey(e: KeyboardEvent) {
    if (!gallery.lightboxOpen) return;
    if (e.key === "Escape") close();
    else if (e.key === "ArrowLeft") navigate(-1);
    else if (e.key === "ArrowRight") navigate(1);
  }

  $effect(() => {
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const actionItems = $derived<ActionSheetItem[]>(
    gallery.selectedImage
      ? [
          {
            label: tt("gallery.send_to_generate", "Send to Generate"),
            onSelect: async () => {
              if (!gallery.selectedImage) return;
              try {
                await sendImageToImg2Img(gallery.selectedImage);
                onSwitchToGenerate?.();
              } catch (e) {
                console.error(e);
              }
            },
          },
          {
            label: tt("gallery.use_for_upscale", "Use for Upscale"),
            onSelect: async () => {
              if (!gallery.selectedImage) return;
              try {
                await sendImageToUpscale(gallery.selectedImage);
                onSwitchToGenerate?.();
              } catch (e) {
                console.error(e);
              }
            },
          },
          {
            label: tt("gallery.copy_image", "Copy image"),
            onSelect: () => gallery.selectedImage && gallery.copyToClipboard(gallery.selectedImage),
          },
          {
            label: tt("gallery.save_as", "Download"),
            onSelect: () => gallery.selectedImage && gallery.saveImageAs(gallery.selectedImage),
          },
          {
            label: tt("gallery.delete", "Delete"),
            destructive: true,
            onSelect: () => {
              if (!gallery.selectedImage) return;
              gallery.deleteImage(gallery.selectedImage);
              close();
            },
          },
        ]
      : [],
  );
</script>

{#if gallery.lightboxOpen}
  <div class="fixed inset-0 z-[60] bg-black flex flex-col tap-highlight-none">
    <!-- Top bar -->
    <div class="absolute top-0 left-0 right-0 z-10 flex items-center justify-between px-2 py-2 safe-top bg-gradient-to-b from-black/70 to-transparent">
      <button
        type="button"
        onclick={close}
        class="touch-target px-3 text-white"
        aria-label={tt("common.aria_close", "Close")}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-6 h-6" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
        </svg>
      </button>
      <div class="text-xs text-white/70">
        {#if currentIndex >= 0}
          {currentIndex + 1} / {images.length}
        {/if}
      </div>
      <button
        type="button"
        onclick={() => (actionOpen = true)}
        class="touch-target px-3 text-white"
        aria-label={tt("common.aria_actions", "Actions")}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-6 h-6" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="1"/><circle cx="12" cy="5" r="1"/><circle cx="12" cy="19" r="1"/>
        </svg>
      </button>
    </div>

    <!-- Image area -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="flex-1 min-h-0 flex items-center justify-center overflow-hidden touch-none select-none"
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
      ondblclick={onDoubleClick}
    >
      {#if gallery.lightboxLoading || !gallery.lightboxUrl}
        <div class="text-white/60 text-sm">{tt("common.loading", "Loading…")}</div>
      {:else}
        <img
          bind:this={imgEl}
          src={gallery.lightboxUrl}
          alt={gallery.selectedImage?.filename ?? ""}
          class="max-w-full max-h-full will-change-transform"
          style="transform: translate({translateX + swipeDX}px, {translateY}px) scale({scale}); transition: {panActive || swipeActive ? 'none' : 'transform 0.2s ease'};"
          draggable="false"
        />
      {/if}
    </div>

    <!-- Bottom action row -->
    <div class="shrink-0 flex items-center justify-around px-2 py-2 safe-bottom bg-gradient-to-t from-black/80 to-transparent">
      <button
        type="button"
        class="touch-target flex flex-col items-center gap-0.5 text-white/90 px-3 disabled:opacity-30"
        disabled={currentIndex <= 0}
        onclick={() => navigate(-1)}
        aria-label={tt("common.aria_previous", "Previous")}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="15 18 9 12 15 6"/>
        </svg>
        <span class="text-[10px]">{tt("common.prev", "Prev")}</span>
      </button>
      <button
        type="button"
        class="touch-target flex flex-col items-center gap-0.5 text-white/90 px-3"
        onclick={async () => {
          if (gallery.selectedImage) {
            try {
              await sendImageToImg2Img(gallery.selectedImage);
              onSwitchToGenerate?.();
            } catch (e) { console.error(e); }
          }
        }}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 12a9 9 0 1 1-9-9c2.5 0 4.8 1 6.5 2.7L21 8"/><polyline points="21 3 21 8 16 8"/>
        </svg>
        <span class="text-[10px]">{tt("gallery.use", "Use")}</span>
      </button>
      <button
        type="button"
        class="touch-target flex flex-col items-center gap-0.5 text-white/90 px-3"
        onclick={() => gallery.selectedImage && gallery.saveImageAs(gallery.selectedImage)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>
        </svg>
        <span class="text-[10px]">{tt("gallery.save_as", "Save")}</span>
      </button>
      <button
        type="button"
        class="touch-target flex flex-col items-center gap-0.5 text-red-400 px-3"
        onclick={() => {
          if (gallery.selectedImage) {
            gallery.deleteImage(gallery.selectedImage);
            close();
          }
        }}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/>
        </svg>
        <span class="text-[10px]">{tt("gallery.delete", "Delete")}</span>
      </button>
      <button
        type="button"
        class="touch-target flex flex-col items-center gap-0.5 text-white/90 px-3 disabled:opacity-30"
        disabled={currentIndex < 0 || currentIndex >= images.length - 1}
        onclick={() => navigate(1)}
        aria-label={tt("common.aria_next", "Next")}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none"
          stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="9 18 15 12 9 6"/>
        </svg>
        <span class="text-[10px]">{tt("common.next", "Next")}</span>
      </button>
    </div>
  </div>

  <MobileActionSheet
    open={actionOpen}
    title={gallery.selectedImage?.filename ?? ""}
    items={actionItems}
    onClose={() => (actionOpen = false)}
  />
{/if}
