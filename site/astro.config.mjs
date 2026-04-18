// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';
import tailwindcss from '@tailwindcss/vite';
import {
	coreToolCount,
	gatewayCount,
	providerCount,
	workspaceVersion,
} from './src/data/project-facts.ts';

const siteUrl = 'https://www.edgecrab.com';
const ogImageUrl = `${siteUrl}/og-image.png`;
const siteDescription = `Rust-native autonomous coding agent and personal assistant. ${coreToolCount} core tools, ${providerCount} LLM providers, ${gatewayCount} messaging gateways, and a single native binary.`;

export default defineConfig({
	site: siteUrl,
	integrations: [
		sitemap({
			changefreq: 'weekly',
			priority: 0.7,
			lastmod: new Date(),
		}),
		starlight({
			title: 'EdgeCrab',
			description: siteDescription,
			logo: {
				light: './src/assets/logo.svg',
				dark: './src/assets/logo-dark.svg',
				replacesTitle: true,
				alt: 'EdgeCrab',
			},
			favicon: '/favicon.svg',
			lastUpdated: true,
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/raphaelmansuy/edgecrab' },
			],
			editLink: {
				baseUrl: 'https://github.com/raphaelmansuy/edgecrab/edit/main/site/',
			},
			customCss: [
				'./src/styles/tokens.css',
				'./src/styles/global.css',
			],
			components: {
				Hero: './src/components/landing/Hero.astro',
				SocialIcons: './src/components/SocialIcons.astro',
				Footer: './src/components/landing/Footer.astro',
			},
			head: [
				{
					tag: 'link',
					attrs: { rel: 'sitemap', href: '/sitemap-index.xml' },
				},
				{
					tag: 'link',
					attrs: { rel: 'preconnect', href: 'https://fonts.googleapis.com' },
				},
				{
					tag: 'link',
					attrs: {
						rel: 'preconnect',
						href: 'https://fonts.gstatic.com',
						crossorigin: true,
					},
				},
				{
					tag: 'link',
					attrs: {
						rel: 'stylesheet',
						href: 'https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500&display=swap',
					},
				},
				{
					tag: 'meta',
					attrs: { property: 'og:type', content: 'website' },
				},
				{
					tag: 'meta',
					attrs: { property: 'og:site_name', content: 'EdgeCrab' },
				},
				{
					tag: 'meta',
					attrs: { property: 'og:image', content: ogImageUrl },
				},
				{
					tag: 'meta',
					attrs: { property: 'og:image:width', content: '1200' },
				},
				{
					tag: 'meta',
					attrs: { property: 'og:image:height', content: '630' },
				},
				{
					tag: 'meta',
					attrs: { property: 'og:image:type', content: 'image/png' },
				},
				{
					tag: 'meta',
					attrs: {
						property: 'og:image:alt',
						content: 'EdgeCrab – Rust-native autonomous coding agent. Blazing-fast TUI. Single static binary.',
					},
				},
				{
					tag: 'meta',
					attrs: { property: 'og:image:secure_url', content: ogImageUrl },
				},
				{
					tag: 'meta',
					attrs: { property: 'og:locale', content: 'en_US' },
				},
				// ── Theme & manifest ──────────────────────────────────────────────
				{
					tag: 'meta',
					attrs: { name: 'theme-color', content: '#EA580C' },
				},
				{
					tag: 'meta',
					attrs: { name: 'color-scheme', content: 'dark light' },
				},
				{
					tag: 'meta',
					attrs: { name: 'msapplication-TileColor', content: '#EA580C' },
				},
				{
					tag: 'link',
					attrs: { rel: 'manifest', href: '/site.webmanifest' },
				},
				{
					tag: 'link',
					attrs: {
						rel: 'apple-touch-icon',
						sizes: '180x180',
						href: '/apple-touch-icon.png',
					},
				},
				// ── JSON-LD structured data ───────────────────────────────────────
				{
					tag: 'script',
					attrs: { type: 'application/ld+json' },
					content: JSON.stringify({
						'@context': 'https://schema.org',
						'@type': 'SoftwareApplication',
						name: 'EdgeCrab',
						applicationCategory: 'DeveloperApplication',
						operatingSystem: 'macOS, Linux, Windows',
						url: siteUrl,
						downloadUrl: 'https://github.com/raphaelmansuy/edgecrab/releases',
						softwareVersion: workspaceVersion,
						description: siteDescription,
						offers: {
							'@type': 'Offer',
							price: '0',
							priceCurrency: 'USD',
						},
						author: {
							'@type': 'Person',
							name: 'Raphaël Mansuy',
							url: 'https://github.com/raphaelmansuy',
						},
						codeRepository: 'https://github.com/raphaelmansuy/edgecrab',
						license: 'https://opensource.org/licenses/Apache-2.0',
						programmingLanguage: 'Rust',
					}),
				},
				// ── Twitter / X Card ─────────────────────────────────────────────
				{
					tag: 'meta',
					attrs: { name: 'twitter:card', content: 'summary_large_image' },
				},
				{
					tag: 'meta',
					attrs: { name: 'twitter:image', content: ogImageUrl },
				},
				{
					tag: 'meta',
					attrs: {
						name: 'twitter:title',
						content: 'EdgeCrab — Rust-native Autonomous Coding Agent',
					},
				},
				{
					tag: 'meta',
					attrs: {
						name: 'twitter:description',
						content: siteDescription,
					},
				},
			],
			sidebar: [
				{
					label: 'Getting Started',
					items: [
						{ label: 'Quick Start', slug: 'getting-started/quick-start' },
						{ label: 'Installation', slug: 'getting-started/installation' },
						{ label: 'Updating & Uninstalling', slug: 'getting-started/updating' },
						{ label: 'FAQ & Troubleshooting', slug: 'getting-started/faq' },
						{ label: 'Learning Path', slug: 'getting-started/learning-path' },
					],
				},
				{
					label: 'User Guide',
					items: [
						{ label: 'CLI Interface', slug: 'user-guide/cli' },
						{ label: 'Configuration', slug: 'user-guide/configuration' },
						{ label: 'Sessions', slug: 'user-guide/sessions' },
						{ label: 'Git Worktrees', slug: 'user-guide/worktrees' },
						{ label: 'Docker Deployment', slug: 'user-guide/docker' },
						{ label: 'Security Model', slug: 'user-guide/security' },
						{ label: 'Checkpoints & Rollback', slug: 'user-guide/checkpoints' },
						{ label: 'Profiles', slug: 'user-guide/profiles' },
						{ label: 'Migrating from Hermes', slug: 'user-guide/migration' },
					],
				},
				{
					label: 'Messaging',
					items: [
						{ label: 'Gateway Overview', slug: 'user-guide/messaging' },
						{ label: 'Telegram', slug: 'user-guide/messaging/telegram' },
						{ label: 'Discord', slug: 'user-guide/messaging/discord' },
						{ label: 'Slack', slug: 'user-guide/messaging/slack' },
						{ label: 'Signal', slug: 'user-guide/messaging/signal' },
						{ label: 'WhatsApp', slug: 'user-guide/messaging/whatsapp' },
					],
				},
				{
					label: 'Features',
					items: [
						{ label: 'Overview', slug: 'features/overview' },
						{ label: 'ReAct Tool Loop', slug: 'features/react-loop' },
						{ label: 'Tools & Toolsets', slug: 'features/tools' },
						{ label: 'Skills System', slug: 'features/skills' },
						{ label: 'Memory', slug: 'features/memory' },
						{ label: 'Context Files', slug: 'features/context-files' },
						{ label: 'TUI Interface', slug: 'features/tui' },
						{ label: 'Cron Jobs', slug: 'features/cron' },
						{ label: 'Browser Automation', slug: 'features/browser' },
						{ label: 'SQLite State & Search', slug: 'features/state' },
					],
				},
				{
					label: 'LLM Providers',
					items: [
						{ label: 'Provider Overview', slug: 'providers/overview' },
						{ label: 'Local Models (Ollama & LM Studio)', slug: 'providers/local' },
					],
				},
				{
					label: 'Integrations',
					items: [
						{ label: 'ACP / VS Code Copilot', slug: 'integrations/acp' },
						{ label: 'Python SDK', slug: 'integrations/python-sdk' },
						{ label: 'Node.js SDK', slug: 'integrations/node-sdk' },
					],
				},
				{
					label: 'Guides & Tutorials',
					items: [
						{ label: 'Building Your First Skill', slug: 'guides/first-skill' },
						{ label: 'Autonomous Coding Workflows', slug: 'guides/coding-workflows' },
						{ label: 'Self-Hosting with Docker', slug: 'guides/self-hosting' },
					],
				},
				{
					label: 'SDK Tutorials',
					badge: { text: 'NEW', variant: 'tip' },
					items: [
						{ label: 'Overview', slug: 'tutorials' },
						{ label: '1. Cost-Aware Code Review', slug: 'tutorials/01-cost-aware-review' },
						{ label: '2. Parallel Research Pipeline', slug: 'tutorials/02-parallel-research' },
						{ label: '3. Multi-Agent Documentation', slug: 'tutorials/03-multi-agent-pipeline' },
						{ label: '4. Session-Aware Support Bot', slug: 'tutorials/04-session-aware-support' },
						{ label: '5. Safe SQL Agent (Custom Tool)', slug: 'tutorials/05-custom-tool-safe-sql' },
					],
				},
				{
					label: 'Developer Guide',
					items: [
						{ label: 'Architecture', slug: 'developer/architecture' },
						{ label: 'Contributing', slug: 'developer/contributing' },
						{ label: 'Releasing', slug: 'developer/releasing' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'CLI Commands', slug: 'reference/cli-commands' },
						{ label: 'Configuration Reference', slug: 'reference/configuration' },
						{ label: 'Slash Commands', slug: 'reference/slash-commands' },
						{ label: 'Environment Variables', slug: 'reference/environment-variables' },
						{ label: 'Changelog', slug: 'changelog' },
					],
				},
			],
		}),
	],
	vite: {
		plugins: [tailwindcss()],
	},
});
