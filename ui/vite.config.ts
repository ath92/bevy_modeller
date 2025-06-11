import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [solid()],
  build: {
    lib: {
      entry: resolve(__dirname, "src/index.tsx"),
      name: "bevy-app-frontend",
      // the proper extensions will be added
      fileName: "index",
    },
  },
});
