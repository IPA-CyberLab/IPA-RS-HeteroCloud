import { readFileSync } from "node:fs";
import path from "node:path";

// Keep font files and licenses on the same origin; no external font requests.
export function siteAssets(consoleRoot: string): Map<string, Buffer> {
  const assets = new Map<string, Buffer>([
    ["/site-assets/site.css", readFileSync(path.join(consoleRoot, "site/site.css"))],
    ["/site-assets/legal-CC-BY-SA-4.0.txt", readFileSync(path.join(consoleRoot, "site/licenses/CC-BY-SA-4.0.txt"))],
  ]);
  for (const name of ["noto-sans", "noto-sans-jp"]) {
    const directory = path.join(consoleRoot, "node_modules/@fontsource-variable", name);
    const css = readFileSync(path.join(directory, "index.css"));
    const prefix = `/site-assets/fonts/${name}/`;
    assets.set(`${prefix}index.css`, css);
    assets.set(`${prefix}LICENSE`, readFileSync(path.join(directory, "LICENSE")));
    for (const match of css.toString().matchAll(/url\(\.\/(files\/[a-z0-9-]+\.woff2)\)/g)) {
      assets.set(`${prefix}${match[1]}`, readFileSync(path.join(directory, match[1])));
    }
  }
  return assets;
}

export function assetContentType(url: string): string {
  if (url.endsWith(".css")) return "text/css; charset=utf-8";
  if (url.endsWith(".woff2")) return "font/woff2";
  return "text/plain; charset=utf-8";
}
