// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import sitemap from '@astrojs/sitemap';
import tailwindcss from '@tailwindcss/vite';

const siteUrl = 'https://www.edgecrab.com';
const ogImageUrl = `${siteUrl}/og-image.png`;

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
			description: 'Rust-native autonomous coding agent. Blazing-fast TUI, ReAct tool loop, multi-provider LLM, ACP protocol, built-in security. Single static binary.',
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
					attrs: { property: 'og:image:type', content: 'image/jpeg' },
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
						content: 'Blazing-fast TUI, ReAct tool loop, multi-provider LLM, ACP protocol. Single static binary. < 50ms startup.',
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
					label: 'Developer Guide',
					items: [
						{ label: 'Architecture', slug: 'developer/architecture' },
						{ label: 'Contributing', slug: 'developer/contributing' },
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
