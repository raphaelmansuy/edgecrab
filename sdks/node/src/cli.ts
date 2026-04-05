#!/usr/bin/env node

/**
 * CLI entry point for the EdgeCrab Node.js SDK.
 *
 * Usage:
 *   edgecrab chat "Hello, what can you do?"
 *   edgecrab chat --model gpt-4 --system "You are a Rust expert" "How to use traits?"
 *   edgecrab models
 *   edgecrab health
 */

import { parseArgs } from 'node:util';
import { EdgeCrabClient, EdgeCrabError } from './client.js';
import { Agent } from './agent.js';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    'base-url':    { type: 'string' },
    'api-key':     { type: 'string' },
    model:         { type: 'string', short: 'm', default: 'anthropic/claude-sonnet-4-20250514' },
    system:        { type: 'string', short: 's' },
    temperature:   { type: 'string', short: 't' },
    stream:        { type: 'boolean', default: false },
    version:       { type: 'boolean', short: 'v' },
    help:          { type: 'boolean', short: 'h' },
  },
});

if (values.version) {
  // Read version from package.json at runtime
  const pkg = await import('../package.json', { with: { type: 'json' } }).catch(() => ({ default: { version: '0.1.0' } }));
  console.log(`edgecrab-sdk ${pkg.default.version}`);
  process.exit(0);
}

const command = positionals[0];
const messageArgs = positionals.slice(1);

if (values.help || !command) {
  console.log(`\
Usage: edgecrab <command> [options] [args...]

Commands:
  chat <message>    Send a message to the agent
  models            List available models
  health            Check API health

Options:
  --base-url <url>    API server URL (env: EDGECRAB_BASE_URL)
  --api-key <key>     Bearer token (env: EDGECRAB_API_KEY)
  -m, --model <id>    Model to use (default: anthropic/claude-sonnet-4-20250514)
  -s, --system <msg>  System prompt
  -t, --temperature   Sampling temperature
  --stream            Stream the response
  -v, --version       Show version
  -h, --help          Show this help
`);
  process.exit(values.help ? 0 : 1);
}

const clientOpts = {
  baseUrl: values['base-url'] ?? process.env.EDGECRAB_BASE_URL,
  apiKey: values['api-key'] ?? process.env.EDGECRAB_API_KEY,
};

try {
  switch (command) {
    case 'chat': {
      const message = messageArgs.join(' ');
      if (!message) {
        console.error('Error: no message provided');
        process.exit(1);
      }

      const agent = new Agent({
        ...clientOpts,
        model: values.model,
        systemPrompt: values.system,
        temperature: values.temperature ? parseFloat(values.temperature) : undefined,
        streaming: values.stream,
        onToken: values.stream ? (token: string) => process.stdout.write(token) : undefined,
      });

      if (values.stream) {
        for await (const token of agent.stream(message)) {
          process.stdout.write(token);
        }
        process.stdout.write('\n');
      } else {
        const reply = await agent.chat(message);
        console.log(reply);
      }
      break;
    }

    case 'models': {
      const client = new EdgeCrabClient(clientOpts);
      const models = await client.listModels();
      for (const m of models) {
        console.log(`  ${m.id}${m.owned_by ? `  (by ${m.owned_by})` : ''}`);
      }
      break;
    }

    case 'health': {
      const client = new EdgeCrabClient(clientOpts);
      const h = await client.health();
      console.log(JSON.stringify(h, null, 2));
      break;
    }

    default:
      console.error(`Unknown command: ${command}`);
      process.exit(1);
  }
} catch (err: unknown) {
  if (err instanceof EdgeCrabError) {
    console.error(`Error: ${err.message}`);
  } else {
    console.error(`Error: ${err instanceof Error ? err.message : String(err)}`);
  }
  process.exit(1);
}
