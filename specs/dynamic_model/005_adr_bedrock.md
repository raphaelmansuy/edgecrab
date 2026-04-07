# ADR 005: AWS Bedrock Discovery

## Status

Accepted with constraints

## Context

The existing `edgequake-llm` Bedrock provider is runtime-oriented. It does not
expose a model-listing method. Bedrock model enumeration lives in the AWS
control-plane API, not the runtime API.

That means EdgeCrab cannot get dynamic Bedrock discovery "for free" by calling
the existing provider.

## Decision

Implement Bedrock discovery through the AWS control-plane SDK, and enable it in
the default workspace build.

Feature switches:

- `edgecrab-core/bedrock-model-discovery`
- propagated by `edgecrab-cli/bedrock-model-discovery`

Why the switches still exist:

- It increases dependency and compile cost.
- Custom downstream builds may still choose to disable Bedrock discovery with
  `--no-default-features`.
- Keeping the feature boundary preserves a clean escape hatch if AWS SDK
  constraints change.

## Discovery behavior

- Use AWS standard credential chain and `AWS_REGION`.
- Call `ListFoundationModels`.
- Include only text output capable models.
- Return provider/model IDs exactly as Bedrock reports them.
- Cache results with a medium TTL.

## Roadblocks

- Bedrock availability is region-dependent.
- Marketplace or account entitlements can change visible inventory.
- Some entries may be technically listable but not invokable without additional
  setup. That is an AWS platform property, not an EdgeCrab heuristic problem.

## Consequences

- Bedrock support exists in default builds, and remains explicit about region
  and entitlement constraints.
- Custom lean builds can still opt out if they do not want AWS SDK compile
  overhead.
