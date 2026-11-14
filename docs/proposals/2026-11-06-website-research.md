# Website research: first page, docs structure, AI readers

Research for the lead, 2026-11-06. No decisions taken. Pages were read through a fetch tool that summarises, so section lists are approximate. Nothing was installed.

## In short

- The restrained sites put one sentence of what-it-is first and one install or start action directly under it: Zig ("general-purpose language and toolchain"), Tauri ("small, fast, secure, cross-platform applications"), Bun, Astro. https://ziglang.org, https://tauri.app, https://bun.sh, https://astro.build
- The shortest pages (Tauri, Zig, Linear, Vercel) have 4 to 11 sections; Bun and Deno run to about 18, mostly benchmarks and API grids. Length is a choice, not a requirement. https://tauri.app, https://vercel.com, https://deno.com
- Art is mostly product: terminal replays, code, UI mockups, charts. The illustrated outliers (Deno's closing banner, Zig's mascots) sit at the bottom or in side sections, never replacing the explanation. https://deno.com, https://ziglang.org, https://linear.app
- The second section is usually "what you get" in three or four short cards (Tauri's four cards, Zed's Fast/Agentic/Collaborative, Astro's three pillars, Linear's three numbered cards), then one real code or product sample. https://tauri.app, https://zed.dev, https://astro.build, https://linear.app
- Diátaxis separates tutorials (learning), how-to guides (a task), reference (facts) and explanation (understanding), and says to keep them apart: a how-to has "no digression, explanation, teaching". It also says to start applying it before fully understanding it. https://diataxis.fr, https://diataxis.fr/how-to-guides/, https://diataxis.fr/application/
- The docs entry page of the best AI-era sites is a short list of "start here" tasks, then a browse-by-area list (Stripe), or a sidebar plus a few featured cards (Ghostty). https://docs.stripe.com, https://ghostty.org/docs
- The AI-reader kit is now standard and small: `/llms.txt`, a `.md` twin of every page, a copy-as-Markdown button, and a skill or setup page an agent can follow. https://llmstxt.org, https://docs.stripe.com/agents.md, https://developers.cloudflare.com/style-guide/components/buttons/index.md
- Several first pages speak to agents directly: Vercel's hero is "Agentic Infrastructure" with a link to an agent playbook at `/get-started.md`; Deno's hero has an "Or let your agent do it" prompt box. https://vercel.com, https://vercel.com/get-started.md, https://deno.com

## Sites looked at

| Site | First line | Second section | Action(s) | Art |
|---|---|---|---|---|
| [Bun](https://bun.sh) | "Bun is a fast JavaScript runtime & toolkit. All in one." | Install commands, then four benchmark charts | Install; quickstart | Terminal replay, charts; no illustration. About 19 sections. |
| [Deno](https://deno.com) | "Better, faster JavaScript." | Ticker of built-in tools, then "Everything just works" with code | Get started; GitHub; "let your agent do it" prompt | Mascot, UI mockups; large illustration only at the closing CTA. About 18 sections. |
| [Zig](https://ziglang.org) | "Zig Programming Language": general-purpose language and toolchain | "A Simple Language" (3 bullets), then Comptime, then adoption in C/C++ | Full overview; code samples | Emoji in hero, two small mascots lower. About 11 short sections. |
| [Tauri](https://tauri.app) | "Tauri 2.0": small, fast, secure, cross-platform apps | Tabbed create-project commands, then four feature cards | Get started | None; sponsor logos fill most of the page. |
| [Zed](https://zed.dev) | "Your last next editor." | Three value cards (Fast, Agentic, Collaborative) | Download; clone source | Product screenshots and video posters. About 12 sections. |
| [Ghostty](https://ghostty.org) | *(unverified: the home page could not be read; docs home is a title, search and featured cards)* | *(unverified)* | Download; build from source ([docs](https://ghostty.org/docs)) | *(unverified)* |
| [Astro](https://astro.build) | "The web framework for content-driven websites." | "What is Astro", three pillars | Get started; `npm create astro@latest` | Background illustration, charts, theme screenshots. Long. |
| [Vercel](https://vercel.com) | "Agentic Infrastructure": for coding agents to ship apps | Three outcome sections each with one customer case study | Deploy now; talk to sales; agent setup guide | No imagery described. About 500 words. |
| [Linear](https://linear.app) | "The product development system for teams and agents" | "A new species of product tool", three numbered cards | Get started; contact sales | UI mockups only. About 9 sections. |
| [Stripe docs](https://docs.stripe.com) | "Explore our guides and examples to integrate Stripe." | "Start here" with agent skills, then nine use-case links | Quickstart; use-case guides | None; text index, then "Browse by product". |

## For AI readers

- **llms.txt.** A Markdown file at `/llms.txt`: an H1 with the site name (the only required part), a blockquote summary, then H2 sections of `[name](url): note` links, with an "Optional" section agents may skip. Pages should also have a `.md` twin, found by `rel="alternate" type="text/markdown"`. https://llmstxt.org
- **Pages as Markdown.** Stripe: append `.md` to any docs URL. https://docs.stripe.com/agents.md. Depot ships the same plus a copy button. https://depot.dev/changelog/2026-05-29-markdown-for-docs-and-ai-agents.md. Fumadocs, which we are using, has `llms()` for `llms.txt` and `llms-full.txt`, per-page `.md` routes, Accept-header negotiation, `MarkdownCopyButton` and `ViewOptionsPopover`. https://fumadocs.dev/docs/integrations/llms
- **Copy / open-in-AI buttons (real examples).** Cloudflare's page header has "Copy as Markdown", "View as Markdown" and "Agent setup" (https://developers.cloudflare.com/agent-setup/). PostHog's page menu copies or views the Markdown, or opens a prompt in ChatGPT or Claude. https://posthog.com/docs/ai-engineering/markdown-llms-txt. Superwall has Copy Markdown and an Open menu for ChatGPT, Claude and Cursor. https://superwall.com/docs/agents/docs-for-ai. A literal "copy a prompt" button I did not find; these send the page, or a prompt wrapping it.
- **A page that tells the agent what to do.** Vercel's `/get-started.md` is a playbook with front matter and three numbered steps (install CLI, add Vercel's skills or plugin, connect the MCP server) and a report template; it says "Perform actions yourself when terminal or file access is available." https://vercel.com/get-started.md. Deno's hero offers "Or let your agent do it". https://deno.com
- **Skills, MCP and plugins from the docs.** Stripe leads its docs with agent skills and a plugin that bundles its MCP server and skills (`stripe agent setup`), with a machine-readable skills catalog at `/.well-known/skills/index.json`. https://docs.stripe.com/agents.md
- **`skill.md` on every docs site.** Mintlify generates `/skill.md` (capabilities, inputs, constraints, workflows), serves it under `/.well-known/agent-skills/index.json`, and installs with `npx skills add <docs-url>`. https://www.mintlify.com/docs/ai/skillmd
- **Repo-level instructions.** `AGENTS.md` is the cross-tool README for agents (setup, build, test, style, PR rules; nearest file wins), read by Codex, Cursor, Zed, Copilot, Gemini CLI and others. https://agents.md. The Agent Skills format is a folder with `SKILL.md`, whose front matter needs `name` and `description`. https://github.com/anthropics/skills
- Not found: Cursor rules documentation was not read; treat Cursor `.mdc` rules as *(unverified)*.

## Skills found

None is specific to a developer-platform front page or docs. Nothing was installed.

| Name | What it says | URL |
|---|---|---|
| `documentation-writer` (github/awesome-copilot) | Acts as a technical writer using Diátaxis; first settle document type, audience, goal and scope, and get an outline approved before writing. The closest fit. | https://github.com/github/awesome-copilot/blob/main/skills/documentation-writer/SKILL.md |
| `doc-coauthoring` (anthropics/skills) | Three stages: gather context, build section by section, then test the doc on a fresh reader with no context and fix the gaps. Aimed at specs and proposals, but the fresh-reader test suits docs. | https://github.com/anthropics/skills/blob/main/skills/doc-coauthoring/SKILL.md |
| `frontend-design` (anthropics/skills) | Start from product, audience and job; typography carries personality; avoid identical SaaS card grids and scattered motion; plan, build, then critique and cut. Relevant to the "restrained" brief. | https://github.com/anthropics/skills/blob/main/skills/frontend-design/SKILL.md |
| `writing-landing-page-copy` (wesleysmits) | Several hero headline options, then goal and audience, section layout, trust signals, CTA refinement. Conversion-marketing flavour; check the source before use. | https://skills.sh/wesleysmits/agent-skills/writing-landing-page-copy, repo https://github.com/wesleysmits/agent-skills |
| `writing-copy` (spencerpauly/awesome-cursor-skills) | Active voice, short sentences; developer docs need a different tone from consumer copy. Seen only through an aggregator summary *(unverified)*. | https://skillselion.com/skills/spencerpauly/awesome-cursor-skills/writing-copy |
| Mintlify skill | Open-source skill that teaches coding agents to build and maintain a Mintlify docs site; tied to Mintlify, so useful as a model, not for Fumadocs *(install line unverified)*. | https://www.mintlify.com/library/mintlify-agent-skill |
| `brand-guidelines`, `internal-comms`, `skill-creator` (anthropics/skills) | Brand colours and type; internal comms formats; skill authoring. Listed to show the repo has nothing for platform docs or landing pages (19 folders, only the first of three pages of listing was readable, so the list may be incomplete). | https://github.com/anthropics/skills/tree/main/skills |

## What this suggests for WaOS

- First page: one sentence saying what WaOS is, and one action under it. Our two audiences differ (a person with an AI who wants an app; an agent that builds), so give each exactly one door and nothing else above the fold, as Vercel does with "Deploy now" plus the agent setup guide.
- Second section: three or four short cards (windows, menus, media, notifications as one idea; runs on every system; sandboxed by construction), then one real program and what it shows on screen. Tauri, Zed and Astro all do this.
- Art: a real screenshot or a short recording of a WaOS program running, not an illustration. Every restrained site's visual is the product.
- Stop at about 6 sections. Leave out benchmarks, logo walls, sponsors, newsletter and testimonials until they exist; Zig and Tauri show a page can be short.
- Docs structure: keep the four Diátaxis kinds apart in navigation: one tutorial (first program), how-to guides per capability (window, menu, media, notification), reference generated from the WIT, and explanation (why components and WebGPU, the sandbox). The docs home is a "start here" list and then browse by area, as Stripe's.
- Ship the AI kit from day one, which Fumadocs largely provides: `llms.txt`, `.md` for every page, `MarkdownCopyButton`, and a `/get-started.md` playbook for an agent (install WaOS, add the skill, build the first program, verify), modelled on Vercel's.
- Publish a WaOS skill (`SKILL.md`) and an `AGENTS.md` template for program authors, discoverable from the docs as Stripe and Mintlify do. For writing the site itself, no existing skill fits; `documentation-writer` plus `frontend-design` are the nearest starting points, and a fresh-reader test (as in `doc-coauthoring`) can check the pages.
- Since agents build the apps, the agent playbook may deserve to be the primary path, and the human tutorial the secondary one. That is a decision for the lead.
