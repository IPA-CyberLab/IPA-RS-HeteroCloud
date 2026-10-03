import { productPages } from "./content.ts";
import { legalPages, templateSource } from "./legal.ts";
import { siteConfig, validateSiteConfig } from "./site.config.ts";
import type { SiteConfig, SiteLink, SitePage, SiteSection } from "./types.ts";

export const navigation: SiteLink[] = [
  { label: "HeteroCloud", href: "/heterocloud/" },
  { label: "HeteroNet", href: "/heteronet/" },
  { label: "技術解説", href: "/technology/" },
  { label: "利用ガイド", href: "/getting-started/" },
  { label: "コンソールへ", href: "/login" },
];

export const escapeHtml = (value: string) => value.replace(/[&<>"']/g, (character) => ({
  "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
})[character]!);

function link({ href, label }: SiteLink, current?: string): string {
  if (!/^\/(?!\/)|^https:\/\/|^mailto:/.test(href)) throw new Error(`Unsupported site link: ${href}`);
  return `<a href="${escapeHtml(href)}"${href === current ? ' aria-current="page"' : ""}>${escapeHtml(label)}</a>`;
}

function section(section: SiteSection): string {
  const { id, title, paragraphs, items, links, table, code } = section;
  return `<section aria-labelledby="${escapeHtml(id)}">
<h2 id="${escapeHtml(id)}">${escapeHtml(title)}</h2>
${paragraphs?.map((text) => `<p>${escapeHtml(text)}</p>`).join("\n") ?? ""}
${items?.length ? `<ul>${items.map((text) => `<li>${escapeHtml(text)}</li>`).join("\n")}</ul>` : ""}
${table ? `<div class="table-scroll" role="region" aria-labelledby="${escapeHtml(id)}" tabindex="0"><table><thead><tr>${table.headings.map((text) => `<th scope="col">${escapeHtml(text)}</th>`).join("")}</tr></thead><tbody>${table.rows.map((row) => `<tr>${row.map((text, index) => index === 0 ? `<th scope="row">${escapeHtml(text)}</th>` : `<td>${escapeHtml(text)}</td>`).join("")}</tr>`).join("\n")}</tbody></table></div>` : ""}
${code ? `<pre><code>${escapeHtml(code)}</code></pre>` : ""}
${links?.length ? `<ul class="related-links">${links.map((item) => `<li>${link(item)}</li>`).join("\n")}</ul>` : ""}
</section>`;
}

export function allPages(config: SiteConfig = siteConfig): SitePage[] {
  validateSiteConfig(config);
  return [...productPages, ...legalPages(config)];
}

export function findPage(pathname: string, config: SiteConfig = siteConfig): SitePage | undefined {
  const path = pathname === "/" ? "/" : `${pathname.replace(/\/$/, "")}/`;
  return allPages(config).find((page) => page.path === path);
}

export function renderPage(page: SitePage, config: SiteConfig = siteConfig): string {
  const draft = page.legal && config.legal.status === "draft";
  const title = page.path === "/" ? page.title : `${page.title} | HeteroCloud`;
  return `<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="description" content="${escapeHtml(page.description)}">
${draft ? '<meta name="robots" content="noindex, follow">' : ""}
<meta name="color-scheme" content="light">
<title>${escapeHtml(title)}</title>
<link rel="stylesheet" href="/site-assets/fonts/noto-sans/index.css">
<link rel="stylesheet" href="/site-assets/fonts/noto-sans-jp/index.css">
<link rel="stylesheet" href="/site-assets/site.css">
</head>
<body>
<a class="skip-link" href="#main">本文へ移動</a>
<header class="site-header">
<a class="site-name" href="/"${page.path === "/" ? ' aria-current="page"' : ""}>HeteroCloud</a>
<nav aria-label="メインナビゲーション"><ul>${navigation.map((item) => `<li>${link(item, page.path)}</li>`).join("")}</ul></nav>
</header>
<main id="main" tabindex="-1">
<article>
<header><h1>${escapeHtml(page.title)}</h1><p class="lead">${escapeHtml(page.lead)}</p></header>
${draft ? '<aside class="policy-status" aria-label="文書の状態"><strong>公開準備中の草案</strong><p>運営者情報、問い合わせ先、料金・保管条件などの確認が残っています。施行済みの規約や、登録時に同意した条件を示すものではありません。</p></aside>' : ""}
${page.legal && config.legal.effectiveDate && !draft ? `<p>施行日：<time datetime="${escapeHtml(config.legal.effectiveDate)}">${escapeHtml(config.legal.effectiveDate)}</time></p>` : ""}
${page.sections.length > 3 ? `<nav class="contents" aria-label="このページの目次"><p>このページの内容</p><ol>${page.sections.map(({ id, title }) => `<li><a href="#${escapeHtml(id)}">${escapeHtml(title)}</a></li>`).join("")}</ol></nav>` : ""}
${page.sections.map(section).join("\n")}
${page.legal ? `<aside class="attribution"><p>原典：<a href="${templateSource}">Automattic / Legalmattic</a>。HeteroCloud 向けに日本語で改変。規約類の文書は <a href="https://creativecommons.org/licenses/by-sa/4.0/">CC BY-SA 4.0</a> で提供します。<a href="/legal/credits/">改変内容・ライセンス</a></p></aside>` : ""}
</article>
</main>
<footer class="site-footer"><p>HeteroCloud / HeteroNetwork</p>
<nav aria-label="フッターナビゲーション"><ul>${[
  { label: "規約・ポリシー", href: "/legal/" },
  { label: "利用規約", href: "/legal/terms/" },
  { label: "プライバシー", href: "/legal/privacy/" },
  { label: "Cookie", href: "/legal/cookies/" },
  { label: "料金・提供条件", href: "/legal/service-conditions/" },
  { label: "お問い合わせ", href: "/contact/" },
  { label: "出典・ライセンス", href: "/legal/credits/" },
].map((item) => `<li>${link(item, page.path)}</li>`).join("")}</ul></nav>
</footer>
</body>
</html>\n`;
}
