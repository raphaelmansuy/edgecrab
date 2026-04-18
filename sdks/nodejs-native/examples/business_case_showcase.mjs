import { Agent, ModelCatalog } from '../index.js';

const MODEL = 'ollama/gemma4:latest';

async function main() {
  console.log('EdgeCrab Node.js SDK — Business Case Showcase');
  console.log(`Model: ${MODEL}`);

  const catalog = new ModelCatalog();
  const est = catalog.estimateCost('openai', 'gpt-5-mini', 2000, 600);
  if (est) {
    console.log(
      `Reference cost estimate for openai/gpt-5-mini: input=$${est[0].toFixed(4)}, output=$${est[1].toFixed(4)}, total=$${est[2].toFixed(4)}`,
    );
  }

  const agent = new Agent({
    model: MODEL,
    maxIterations: 4,
    quietMode: true,
    skipContextFiles: true,
    instructions:
      'You are an operations copilot for a growth-stage software company. Be concise, structured, and business-minded.',
  });

  await agent.memory.write(
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
    console.log(reply);
  }

  console.log('\n=== Batch customer quote summaries ===');
  const quotes = await agent.batch([
    "Summarize this quote in one sentence: 'We love the workflow builder but permissions are still confusing.'",
    "Summarize this quote in one sentence: 'The rollout was smooth and our ops team saved hours every week.'",
    "Summarize this quote in one sentence: 'We need stronger reporting before we expand to more teams.'",
  ]);

  quotes.forEach((item, idx) => {
    console.log(`${idx + 1}. ${String(item).replace(/\n/g, ' ')}`);
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
