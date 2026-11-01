// Bu web görünümünün hangi pencere olduğu: ana pencere ya da ikincil (yeni ileti / okuma).
import { getCurrentWindow } from "@tauri-apps/api/window";

function label(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
}

export const WINDOW_LABEL = typeof window === "undefined" ? "main" : label();
export const IS_MAIN = WINDOW_LABEL === "main";
