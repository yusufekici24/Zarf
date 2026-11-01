// Pencere odak durumu: Outlook gibi etkin pencerenin çerçevesi mavi, değilse gri.
import { getCurrentWindow } from "@tauri-apps/api/window";

class WindowFocus {
  focused = $state(true);
  private started = false;

  start() {
    if (this.started || typeof window === "undefined") return;
    this.started = true;
    try {
      const w = getCurrentWindow();
      w.isFocused().then((f) => (this.focused = f)).catch(() => {});
      void w.onFocusChanged((e) => (this.focused = e.payload)).catch(() => {});
    } catch {
      /* tarayıcıda */
    }
  }
}

export const windowFocus = new WindowFocus();
