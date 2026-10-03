import { mkdir, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import { siteAssets } from "./assets.ts";
import { allPages, renderPage } from "./render.ts";

const root = path.resolve(import.meta.dirname, "..");
const output = path.join(root, "dist");
// Vite builds the console first. The server uses this distinct entry for SPA
// deep links, while index.html becomes the public, JavaScript-free homepage.
await rename(path.join(output, "index.html"), path.join(output, "console.html"));
for (const page of allPages()) {
  const directory = path.join(output, page.path);
  await mkdir(directory, { recursive: true });
  await writeFile(path.join(directory, "index.html"), renderPage(page));
}
for (const [url, contents] of siteAssets(root)) {
  const destination = path.join(output, url);
  await mkdir(path.dirname(destination), { recursive: true });
  await writeFile(destination, contents);
}
console.log(`Generated ${allPages().length} public pages and self-hosted Noto Sans fonts.`);
