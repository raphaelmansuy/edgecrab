# Task Log: thought_signature pending_sig fix

## Actions
- Fixed compile errors: added `thought_signature: None` to two test `ToolCall` struct literals (gemini.rs lines 2778, 2787, 2893)
- Added `pending_sig: Arc<Mutex<Option<String>>>` alongside `fn_call_index` in `chat_with_tools_stream` to carry thoughtSignature from non-FC Parts across SSE chunks
- Added matching `pending_sig: Option<String>` in non-streaming `chat_with_tools` parts loop
- Removed debug `eprintln!` calls added during investigation
- Committed as `b0f1ffe`

## Decisions
- Per official Gemini API spec: Gemini 3 puts thoughtSignature on the functionCall Part (mandatory); Gemini 2.5 puts it on the FIRST Part regardless of type (thinking Part, optional)
- For Gemini 2.5: capture sig from thinking Part, store in pending_sig, apply to next functionCall Part
- For streaming edge case: pending_sig carries across SSE chunks (Arc<Mutex>)
- Putting sig on functionCall Part when echoing (even for Gemini 2.5) is acceptable because Gemini 2.5 signature is optional

## Next Steps
- E2E test with actual Gemini 3 model to confirm HTTP 400 is resolved
- Monitor for Gemini 2.5 behavior with multi-step tool calls

## Lessons
- Gemini API delivers thoughtSignature on DIFFERENT Parts for different model versions; always capture from any Part and carry forward to the next functionCall
