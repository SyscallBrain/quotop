# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Added

- Container image (`ghcr.io/syscallbrain/quotop`, amd64 and arm64) and a
  `Dockerfile` to build it from source.

## 0.1.0 - 2026-09-27

First public release.

### Added

- Terminal UI showing the remaining balance, credits and quotas of 26 services:
  Tavily, Firecrawl, Exa, Serper, Brave Search, Jina, OpenRouter, DeepSeek,
  MiniMax, xAI, Groq, Gemini, Mistral, Cerebras, Ollama Cloud, OpenCode Go,
  Claude, ElevenLabs, Deepgram, fal.ai, Composio, Context7, Twilio, Pushover,
  GitHub and X API.
- Parallel refresh on start and every 15 minutes, never spending paid quota
  automatically; readings cached and shown immediately on the next start.
- Detail panel with exact values, local reset times, endpoint and last error.
- Services & keys screen: choose which services appear (by default, those with
  a key) and type a key that is saved to the key file with mode `600`.
- Preferences screen: language, 8 colour themes and 3 bar styles, plus
  per-colour overrides in `config.toml`.
- English and Portuguese (Portugal) interface, with translations in
  `locales/`.
- `--json` output for scripts, with `--include-paid`.
- Per-service thresholds in the meter's own units.
- Leak guard keeping keys out of the cache, the JSON output and error messages.

