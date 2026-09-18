# Changelog

## Unreleased

- Implemented. `OpenAiCompatProvider` speaks `POST /v1/chat/completions` to
  ollama, vllm, llama.cpp's server, LM Studio and any hosted gateway with the
  same shape, over an injectable `ChatTransport`.
- Streaming chunks map to `ModelEvent`, including tool-call fragments keyed by
  `index`, exposed reasoning under both spellings, and usage with cached
  prompt tokens.
- An unknown `finish_reason` is an error, never `EndTurn`.
- A 400 whose message is a context overflow becomes `ContextTooLong`, so the
  kernel compacts instead of failing the turn.
- Tests replay committed `.sse` files through `RecordedTransport` and open no
  socket.
