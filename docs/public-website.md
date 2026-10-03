# Public website and design handoff

The root URL serves an introduction instead of immediately requiring login.
All public pages are static HTML, readable without JavaScript or an API call.
Console deep links and API endpoints continue to use their existing URLs.
After OIDC sign-in, `/console` handles the saved destination (including CLI
device authorization) and otherwise opens `/overview`.

Static responses, including HEAD and HTTP 304 responses, send
`Cache-Control: no-cache` so browsers revalidate documents after a deployment.
The console's `/` route is also public: if browser history restores the SPA
there, it loads `/?site=public` as a document before checking any session.
Links back from login/registration use that same URL to bypass root HTML cached
before the public website was deployed. `/console` remains authenticated.

## Edit and preview

```sh
cd apps/console
npm ci
npm run dev
```

Open `http://localhost:4173/`. Production output can be inspected with
`npm run build && npm run preview -- --host 127.0.0.1`.
Node 22.18+ or 24 is required for the TypeScript build script; the release
workflow and Dockerfile use Node 24. No new hosting service is necessary.

- `site/content.ts`: introduction, product descriptions, technology, setup.
- `site/legal.ts`: policy and contact copy, with scoped CC BY-SA attribution.
- `site/render.ts`: semantic HTML, navigation, headings, tables and footer.
- `site/site.css`: intentionally small typography/spacing baseline for design.
- `site/site.config.ts`: public operator details and policy publication state.
- `site/build.ts`: public pages and local font assets in the console artifact.

Use Noto Sans for Latin and Noto Sans JP for Japanese. Fonts are self-hosted,
with `font-display: swap`. Preserve keyboard focus, the skip link, heading
hierarchy, responsive table scrolling and the existing same-origin CSP.
No production hostname is embedded in navigation or installation examples.
Public content contains no analytics, trackers or runtime JavaScript.

The build first saves Vite's SPA entry as `dist/console.html`, then writes the
public `dist/index.html`. Deploy the entire artifact with the matching Rust
server. The server also supports older console-only artifacts. Do not use a
static host's default `index.html` SPA fallback for `/login`, `/overview`,
`/cli/authorize`, etc.; the fallback must be **console.html**.

## Public pages

`/`, `/heterocloud/`, `/heteronet/`, `/technology/`, `/getting-started/`,
`/legal/`, `/legal/terms/`, `/legal/privacy/`, `/legal/acceptable-use/`,
`/legal/cookies/`, `/legal/service-conditions/`, `/legal/credits/`, `/contact/`.

## Policy publication

The initial documents are visibly marked as drafts and excluded from search
indexing. They do not introduce a registration consent claim. Before making
them effective, the operator needs to review the actual service practices and
fill in the public operator name, address, representative where applicable,
contact email/form, fees and effective date in `site.config.ts`. In particular,
confirm retention/deletion practices, processors/regions, incident handling,
service limits and the wording of liability and privacy clauses. The template
does not establish that these practices are already in operation.

Setting `legal.status` to `published` validates the basic required fields.
Paid service publication additionally needs actual pricing, payment, delivery,
cancellation/refund and legally required seller disclosures; the current
generator deliberately rejects an incomplete paid-service configuration.
Document provenance and the license scope are in `site/NOTICE.md`.

## Verification

`npm test -- site/site.test.ts src/features/auth/auth-return.test.ts
src/features/auth/auth-pages.test.tsx` checks public links, local font subsets,
draft attribution and authentication return paths. The API's `console_files`
tests check public HTML, console deep links and older artifacts without a
database. A build verifies that all pages and font licenses are packaged.
