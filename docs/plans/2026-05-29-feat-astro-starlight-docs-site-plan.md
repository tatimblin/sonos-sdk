---
title: "feat: Astro Starlight Documentation Site on GitHub Pages"
type: feat
status: active
date: 2026-05-29
---

# Astro Starlight Documentation Site

## Overview

Add a user-facing documentation site to sonos-sdk using Astro Starlight, deployed automatically to GitHub Pages at `https://tatimblin.github.io/sonos-sdk/`. The site documents the SDK's property-centric API (get/fetch/watch), provides a quick start guide, and includes CLI tool documentation.

## Problem Statement / Motivation

The SDK currently relies on docs.rs for API reference and GitHub-rendered Markdown for architecture docs. There is no unified, searchable, user-friendly documentation site that serves both SDK users (Rust developers) and CLI users (anyone with a terminal). A dedicated docs site improves discoverability, reduces onboarding friction, and gives the project a professional appearance.

## Proposed Solution

An Astro Starlight static site living in a dedicated `website/` directory at the repo root, deployed via GitHub Actions using `withastro/action@v6`. The site uses pnpm as its package manager.

### Why `website/` instead of `docs/`

The existing `docs/` directory contains internal developer documentation (specs, brainstorms, plans, STATUS.md) referenced throughout CLAUDE.md, AGENTS.md, and CONTRIBUTING.md. Placing the Astro project in a separate `website/` directory avoids:
- Conflicts with existing internal docs
- Risk of accidentally publishing internal planning documents
- A large cross-reference refactoring effort

### CLI Docs: Direct Authoring (No Submodule)

The sonos-cli repo's `docs/` directory contains only internal files (brainstorms, plans). All user-facing CLI documentation lives in its README.md. Rather than adding a git submodule pointing at a non-existent docs structure, CLI documentation will be authored directly within the docs site, sourced from the sonos-cli README content. A submodule can be introduced later if sonos-cli gains a dedicated user-docs directory.

## Technical Approach

### Directory Structure

```
sonos-sdk/
├── website/                    <-- Astro Starlight project root
│   ├── astro.config.mjs
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── tsconfig.json
│   ├── public/
│   │   └── favicon.svg
│   └── src/
│       ├── assets/
│       │   └── logo.svg        (optional)
│       ├── content/
│       │   └── docs/
│       │       ├── index.mdx
│       │       ├── getting-started/
│       │       │   ├── installation.mdx
│       │       │   └── quick-start.mdx
│       │       ├── guides/
│       │       │   ├── architecture.mdx
│       │       │   ├── properties.mdx
│       │       │   └── cookbook/
│       │       │       ├── control-playback.mdx
│       │       │       ├── monitor-volume.mdx
│       │       │       └── group-management.mdx
│       │       ├── cli/
│       │       │   ├── index.mdx
│       │       │   └── commands.mdx
│       │       └── troubleshooting/
│       │           ├── firewall.mdx
│       │           └── discovery.mdx
│       └── content.config.ts
├── .github/workflows/
│   ├── ci.yml                  (existing, unchanged)
│   └── deploy-docs.yml         (new)
└── ...existing files...
```

### Astro Configuration

```javascript
// website/astro.config.mjs
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://tatimblin.github.io',
  base: '/sonos-sdk',
  integrations: [
    starlight({
      title: 'Sonos SDK',
      social: {
        github: 'https://github.com/tatimblin/sonos-sdk',
      },
      editLink: {
        baseUrl: 'https://github.com/tatimblin/sonos-sdk/edit/main/website/',
      },
      sidebar: [
        {
          label: 'Getting Started',
          items: [
            { slug: 'getting-started/installation' },
            { slug: 'getting-started/quick-start' },
          ],
        },
        {
          label: 'Guides',
          items: [
            { slug: 'guides/architecture' },
            { slug: 'guides/properties' },
            {
              label: 'Cookbook',
              autogenerate: { directory: 'guides/cookbook' },
            },
          ],
        },
        {
          label: 'CLI',
          items: [
            { slug: 'cli' },
            { slug: 'cli/commands' },
          ],
        },
        {
          label: 'Troubleshooting',
          autogenerate: { directory: 'troubleshooting' },
        },
      ],
    }),
  ],
});
```

### Sidebar Tiers (4 Groups)

1. **Getting Started** — Installation, Quick Start (10-line working example)
2. **Guides** — Architecture, Properties (get/fetch/watch), Cookbook recipes
3. **CLI** — Installation, Command Reference (from sonos-cli README)
4. **Troubleshooting** — Firewall/UPnP issues, Discovery problems

### GitHub Actions Workflow

```yaml
# .github/workflows/deploy-docs.yml
name: Deploy Docs

on:
  push:
    branches: [main]
    paths:
      - 'website/**'
  workflow_dispatch:

concurrency:
  group: pages-deploy
  cancel-in-progress: true

permissions:
  contents: read
  pages: write
  id-token: write

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: withastro/action@v6
        with:
          path: ./website
          node-version: 22

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
```

### Key Technical Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Directory name | `website/` | Avoids conflict with existing `docs/` |
| Package manager | pnpm | Fast installs, strict resolution, auto-detected by withastro/action |
| CLI docs approach | Direct authoring | sonos-cli has no structured docs directory; submodule adds complexity for no gain |
| Deploy trigger | Path-filtered (`website/**`) | Avoids unnecessary builds when only Rust code changes |
| Node.js version | 22 | LTS, pinned in workflow for reproducibility |
| Search | Pagefind (default) | Zero-config, fully static, included with Starlight |

## Implementation Phases

### Phase 1: Scaffold and Deploy (MVP)

Set up the Astro project, write the landing page, and get GitHub Pages deployment working.

**Tasks:**
- [x] `website/` — Initialize Starlight project with `pnpm create astro --template starlight`
- [x] `website/astro.config.mjs` — Configure site/base URLs, sidebar structure
- [x] `website/src/content/docs/index.mdx` — Hero landing page with tagline and quick links
- [x] `website/src/content/docs/getting-started/quick-start.mdx` — 10-line SonosSystem::new() example
- [x] `website/src/content/docs/getting-started/installation.mdx` — Cargo.toml dependency + feature flags
- [x] `.github/workflows/deploy-docs.yml` — GitHub Actions workflow
- [x] `.gitignore` — Add `node_modules/`, `.astro/`, `website/dist/`
- [ ] Enable GitHub Pages (Actions source) in repo settings
- [ ] Verify site loads at `https://tatimblin.github.io/sonos-sdk/`

**Success criteria:** Site is live, navigable, and search works on at least 2 pages.

### Phase 2: Core Content

Write the guides that differentiate this SDK.

**Tasks:**
- [x] `website/src/content/docs/guides/architecture.mdx` — Layer diagram, crate responsibilities, data flow
- [x] `website/src/content/docs/guides/properties.mdx` — get/fetch/watch patterns with code examples
- [x] `website/src/content/docs/guides/cookbook/control-playback.mdx` — Play, pause, stop, seek
- [x] `website/src/content/docs/guides/cookbook/monitor-volume.mdx` — Reactive volume watching
- [x] `website/src/content/docs/guides/cookbook/group-management.mdx` — Join, leave, enumerate groups

**Success criteria:** A new user can understand the API design and copy working code snippets.

### Phase 3: CLI and Troubleshooting

Round out the site with CLI reference and common issues.

**Tasks:**
- [x] `website/src/content/docs/cli/index.mdx` — Install via Homebrew/cargo/binary, quick start
- [x] `website/src/content/docs/cli/commands.mdx` — Full command table with flags and examples
- [x] `website/src/content/docs/troubleshooting/firewall.mdx` — UPnP event blocking, polling fallback
- [x] `website/src/content/docs/troubleshooting/discovery.mdx` — SSDP/multicast, Wi-Fi isolation, no speakers found

**Success criteria:** Users can find answers to the two most common support questions without filing issues.

### Phase 4: Polish

- [ ] Add favicon and optional logo
- [ ] Add custom 404 page
- [ ] Link docs site from README.md and crates.io metadata
- [ ] Add "API Reference" external link in sidebar pointing to docs.rs/sonos-sdk

## System-Wide Impact

**Interaction with existing CI:** The new `deploy-docs.yml` is independent of `ci.yml`. Path filtering ensures they don't trigger unnecessarily for each other's changes. PRs that touch both Rust and docs will trigger both workflows, which is correct.

**Repository size:** pnpm-lock.yaml adds ~200KB. The `website/` directory with content adds another ~50KB. Node modules are gitignored.

**Developer workflow:** Contributors only need Node.js/pnpm if they're editing docs locally. Rust development is unaffected.

## Acceptance Criteria

### Functional

- [ ] Site builds and deploys to `https://tatimblin.github.io/sonos-sdk/`
- [ ] All internal links work (no 404s)
- [ ] Pagefind search returns results for "volume", "watch", "firewall"
- [ ] Dark mode toggle works
- [ ] Mobile layout is responsive and navigable
- [ ] Edit links point to correct GitHub file paths

### Non-Functional

- [ ] Deploy completes in under 3 minutes
- [ ] Lighthouse performance score > 90
- [ ] No broken asset paths (CSS, JS, fonts load correctly under `/sonos-sdk/` base)

## Dependencies & Risks

| Risk | Mitigation |
|------|------------|
| Base path misconfiguration breaks all links | Verify in Phase 1 before writing content |
| Docs drift from SDK changes | Single-commit workflow: update code and docs together |
| GitHub Pages not enabled | `workflow_dispatch` trigger allows manual deploy after enabling |
| pnpm version drift | Pin via `packageManager` field in package.json |

## Sources & References

### Internal References
- SDK public API: `sonos-sdk/src/lib.rs:1-62` (doc comments with examples)
- Existing architecture docs: `docs/SUMMARY.md`
- Property docs: `docs/watchable-properties.md`
- CLI source material: `../sonos-cli/README.md`

### External References
- Astro Starlight docs: https://starlight.astro.build
- withastro/action: https://github.com/withastro/action
- GitHub Pages deployment: https://docs.github.com/en/pages
