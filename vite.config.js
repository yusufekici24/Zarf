import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
import fluentMapJson from "./src/lib/icons/fluent-map.js";
/** @type {Record<string, string>} */
const fluentMap = fluentMapJson;

/** "@lucide/svelte/icons/<ad>" içe aktarmalarını Fluent simgelerine çevirir (eşlemesi olanlar) */
function fluentIcons() {
  const PREFIX = "\0fluent-icon:";
  return {
    name: "lynora-fluent-icons",
    enforce: "pre",
    /** @param {string} source */
    resolveId(source) {
      const m = source.match(/^@lucide\/svelte\/icons\/([a-z0-9-]+?)(?:\.svelte|\.js)?$/);
      if (m && fluentMap[m[1]]) return PREFIX + m[1];
    },
    /** @param {string} id */
    load(id) {
      if (!id.startsWith(PREFIX)) return;
      const file = fluentMap[id.slice(PREFIX.length)];
      return `import C from "/src/lib/icons/FluentIcon.svelte";
import svg from "@fluentui/svg-icons/icons/${file}_20_regular.svg?raw";
import { fluentWrap } from "/src/lib/icons/wrap.js";
export default fluentWrap(C, svg);`;
    },
  };
}
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [fluentIcons(), tailwindcss(), sveltekit()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
