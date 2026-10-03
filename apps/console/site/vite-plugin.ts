import path from "node:path";
import { readFileSync } from "node:fs";
import type { Plugin } from "vite";
import { assetContentType, siteAssets } from "./assets.ts";
import { findPage, renderPage } from "./render.ts";

export function publicSite(): Plugin {
  return {
    name: "heterocloud-public-site",
    configureServer(server) {
      const assets = siteAssets(path.resolve(server.config.root));
      const stylesheet = path.resolve(server.config.root, "site/site.css");
      server.watcher.add(stylesheet);
      server.watcher.on("change", (file) => {
        if (file === stylesheet) {
          assets.set("/site-assets/site.css", readFileSync(stylesheet));
          server.ws.send({ type: "full-reload" });
        }
      });
      server.middlewares.use((request, response, next) => {
        if (request.method !== "GET" && request.method !== "HEAD") return next();
        const pathname = new URL(request.url ?? "/", "http://localhost").pathname;
        const page = findPage(pathname);
        const asset = assets.get(pathname);
        if (!page && !asset) return next();
        response.setHeader("Content-Type", page ? "text/html; charset=utf-8" : assetContentType(pathname));
        response.setHeader("Cache-Control", "no-cache");
        response.end(request.method === "HEAD" ? undefined : page ? renderPage(page) : asset);
      });
    },
    configurePreviewServer(server) {
      // Vite's default SPA fallback points to index.html, now the public site.
      // Match the Rust server's fallback for console deep links in preview.
      server.middlewares.use((request, _response, next) => {
        if (request.method === "GET" || request.method === "HEAD") {
          const url = new URL(request.url ?? "/", "http://localhost");
          const pathname = url.pathname;
          if (!findPage(pathname) && !path.posix.extname(pathname)
            && !pathname.startsWith("/site-assets/") && !pathname.startsWith("/api/")) {
            request.url = `/console.html${url.search}`;
          }
        }
        next();
      });
    },
  };
}
