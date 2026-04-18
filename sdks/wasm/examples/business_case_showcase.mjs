import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { Agent } = require('../pkg-node/edgecrab_wasm.js');

const MODEL = 'ollama/gemma4:latest';
const BASE_URL = process.env.OPENAI_BASE_URL || process.env.ANTHROPIC_BASE_URL || 'http://localhost:11434/v1';
const API_KEY = process.env.OPENAI_API_KEY || process.env.ANTHROPIC_AUTH_TOKEN || 'ollama';

async function main() {
  console.log('EdgeCrab WASM SDK — Business Case Showcase');
  console.log(`Model: ${MODEL}`);

  const agent = new Agent(MODEL, {
    baseUrl: BASE_URL,
    apiKey: API_KEY,
    maxIterations: 4,
    instructions:
      'You are an operations copilot for a growth-stage software company. Be concise, structured, and business-minded.',
  });

  agent.memory.write(
    'memory',
    'Company context: AcmeCloud sells workflow automation to mid-market SaaS teams. Priority metrics are churn, expansion revenue, and support resolution speed.',
  );

  const scenarios = [
    {
      title: 'Support triage',
      prompt:
        "A customer says: 'Our onboarding export is failing and we need a fix before tomorrow morning.' Reply with a severity, owner, and next action.",
    },
    {
      title: 'Executive brief',
      prompt:
        'Turn these notes into a short VP-ready update: churn stable, enterprise pipeline up 18%, support backlog down 12%, one release risk in payments.',
    },
    {
      title: 'Sales preparation',
      prompt:
        'Create a short account brief for a renewal call with a customer who wants better audit logs, faster support, and predictable pricing.',
    },
  ];

  for (const { title, prompt } of scenarios) {
    console.log(`\n=== ${title} ===`);
    const reply = await agent.chat(prompt);
    console.log(String(reply).replace(/\n/g, ' '));
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
