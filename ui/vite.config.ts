import { defineConfig } from "vite";
export default defineConfig({
  base: "/",
  build: { chunkSizeWarningLimit: 4500, rollupOptions: { input: {desktop: "index.html", mobile: "mobile.html"} } },
  server: { proxy: { "/api": "http://127.0.0.1:7878" } },
});
