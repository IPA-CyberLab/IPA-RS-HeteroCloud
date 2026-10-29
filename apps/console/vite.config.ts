import path from "node:path";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";
import { publicSite } from "./site/vite-plugin.ts";

export default defineConfig({
  plugins: [publicSite(), tailwindcss(), react()],
  resolve: {
    alias: {
      "@": path.resolve(import.meta.dirname, "./src"),
    },
  },
  server: {
    port: 4173,
  },
  test: {
    include: ["src/**/*.test.{ts,tsx}", "site/**/*.test.ts"],
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
    css: true,
    globals: true,
    restoreMocks: true,
    testTimeout: 15_000,
  },
});
