# Smart Model Routing

Verified against:
- `crates/edgecrab-core/src/model_router.rs`
- `crates/edgecrab-core/src/model_catalog.rs`

EdgeCrab can route simple turns to a cheaper model while leaving complex turns on the primary model.

## Decision path

```text
user message
  -> classify_message()
  -> simple?
     yes -> cheap model route
     no  -> primary model route
```

## What makes a message "complex"

Any of the following keeps the primary model:

- too long
- too many words
- multiline input
- code fences or inline code
- URLs
- complex keywords such as `debug`, `implement`, `review`, `tool`, `docker`, `compile`, `fix`

If none of those checks fire, the message is treated as simple.

## Catalog behavior

`ModelCatalog` is the single source of truth for provider and model listings. It loads:

- embedded defaults from `model_catalog_default.yaml`
- user overrides from `~/.edgecrab/models.yaml`

The catalog is cached in a `OnceLock<RwLock<...>>` and can be reloaded explicitly.

## Practical takeaway

Routing is intentionally conservative. It only opts into the cheap model when the message is short and obviously low-risk.
