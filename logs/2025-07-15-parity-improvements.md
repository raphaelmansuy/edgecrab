# Task Log: Hermes-Agent Parity Improvements

## Actions
- Added SSE streaming support to API server (`/v1/chat/completions` with `stream: true`)
- Added `GET /v1/models` endpoint returning edgecrab model entry
- Added `GET /health` endpoint returning `{"status": "ok"}`
- Added `TtsConfig`, `SttConfig`, `VoiceConfig`, `HonchoConfig`, `AuxiliaryConfig` to config.rs
- Added `reasoning_effort` config field
- Added env overrides for TTS provider, voice, honcho, and reasoning effort
- Added ElevenLabs TTS provider with full API integration
- Made TTS backend selection config-driven via `EDGECRAB_TTS_PROVIDER`
- Added clipboard image paste support (saves clipboard RGBA as PNG)
- Added `pending_images` field to App for injecting images into agent prompts
- Added minimal PNG encoder (no external image deps)
- Added `/voice tts <text>` subcommand for immediate text-to-speech
- Verified honcho_context, honcho_profile tools already implemented

## Decisions
- Used stored DEFLATE for PNG encoding to avoid adding image crate dependency
- SSE streaming emits word-boundary chunks from full response (agent doesn't stream per-token to API)
- Clipboard images saved to `~/.edgecrab/images/` and referenced in prompts for vision_analyze

## Next Steps
- STT (speech-to-text) requires native audio recording — deferred as Rust doesn't have easy whisper bindings
- Cloud honcho sync could be enhanced with actual HTTP CRUD operations
- Real per-token SSE streaming would require gateway-level streaming support

## Lessons
- arboard crate already supports `get_image()` — just need to wire it
- axum 0.7 has built-in SSE support via `axum::response::sse`
- async-stream crate makes building SSE streams ergonomic
