import path from "node:path";
import { describe, expect, it } from "vitest";
import { siteAssets } from "./assets.ts";
import { allPages, findPage, renderPage } from "./render.ts";
import { siteConfig, validateSiteConfig } from "./site.config.ts";

const root = path.resolve(import.meta.dirname, "..");
const assets = siteAssets(root);

describe("public website", () => {
  it("renders public pages without scripts or an authentication dependency", () => {
    for (const page of allPages()) {
      const document = new DOMParser().parseFromString(renderPage(page), "text/html");
      expect(document.documentElement.lang).toBe("ja");
      expect(document.querySelectorAll("h1")).toHaveLength(1);
      expect(document.querySelector("main")?.textContent).toContain(page.lead);
      expect(document.querySelector("script, iframe, form")).toBeNull();
      const ids = [...document.querySelectorAll("[id]")].map((node) => node.id);
      expect(new Set(ids).size).toBe(ids.length);
      expect(findPage(page.path.replace(/\/$/, "") || "/")).toEqual(page);
      expect(renderPage(page)).not.toContain("heterocloud.mizuame.app");
      for (const element of document.querySelectorAll<HTMLAnchorElement | HTMLLinkElement>("a[href], link[href]")) {
        const href = element.getAttribute("href")!;
        if (href.startsWith("#")) expect(document.getElementById(href.slice(1))).not.toBeNull();
        else if (href.startsWith("/")) {
          const url = new URL(href, "https://cloud.example.com");
          const destination = findPage(url.pathname);
          expect(Boolean(destination || assets.has(url.pathname) || ["/login", "/cli"].includes(url.pathname)), href).toBe(true);
          if (url.hash && destination) {
            const target = new DOMParser().parseFromString(renderPage(destination), "text/html");
            expect(target.getElementById(url.hash.slice(1)), href).not.toBeNull();
          }
        }
        if (element.tagName === "LINK") expect(href).toMatch(/^\/site-assets\//);
      }
    }
  });

  it("ships every font subset referenced by CSS, including Japanese glyphs", () => {
    for (const [url, buffer] of assets) {
      if (!url.endsWith("index.css")) continue;
      const references = [...buffer.toString().matchAll(/url\(([^)]+)\)/g)];
      expect(references.length).toBeGreaterThan(0);
      for (const [, reference] of references) {
        expect(assets.get(path.posix.join(path.posix.dirname(url), reference))?.length).toBeGreaterThan(0);
      }
    }
    const css = assets.get("/site-assets/fonts/noto-sans-jp/index.css")!.toString();
    const ranges = [...css.matchAll(/U\+([0-9a-f]+)(?:-([0-9a-f]+))?/gi)];
    for (const character of "日本語") {
      const point = character.codePointAt(0)!;
      expect(ranges.some(([, start, end]) => point >= parseInt(start, 16) && point <= parseInt(end ?? start, 16))).toBe(true);
    }
  });

  it("labels drafts and attributes adapted policies without affecting product indexing", () => {
    for (const page of allPages()) {
      const document = new DOMParser().parseFromString(renderPage(page), "text/html");
      expect(Boolean(document.querySelector('[name="robots"][content="noindex, follow"]'))).toBe(Boolean(page.legal));
      if (page.legal) {
        expect(document.body.textContent).toContain("公開準備中の草案");
        expect(document.body.textContent).toContain("Automattic / Legalmattic");
        expect(document.querySelector('a[href="https://creativecommons.org/licenses/by-sa/4.0/"]')).not.toBeNull();
      }
    }
    expect(() => validateSiteConfig({ ...siteConfig, legal: { ...siteConfig.legal, status: "published" } })).toThrow();
  });

  it("escapes operator information and validates contact links", () => {
    const config = structuredClone(siteConfig);
    config.operator.name = '<script>alert("operator")</script>';
    const html = renderPage(allPages(config).find((page) => page.path === "/contact/")!, config);
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
    config.operator.contactUrl = "javascript:alert(1)";
    expect(() => allPages(config)).toThrow();
  });
});
