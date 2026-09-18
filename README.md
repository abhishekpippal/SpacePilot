# SpacePilot

SpacePilot is a local-first desktop storage analyzer for Windows and macOS. It scans selected folders using its Rust native layer, presents real usage data, finds SHA-256-confirmed duplicates, suggests conservative cleanup candidates, and moves explicitly selected files to the platform trash.

## Privacy

File contents never leave the device. Scanning, analysis, duplicate hashing, and cleanup work offline. An optional Hatchable AI integration is intentionally not enabled in this starter release; when connected, it must receive aggregate scan metadata only, never files or file contents.

## Development

Install Node 20+ and Rust stable, then run `npm install`, `npm run check`, `npm run build`, and `npm run tauri dev`. The Rust dependencies are declared in `src-tauri/Cargo.toml`; run `cargo fmt --check`, `cargo test`, and `cargo check` there.

`package.json` and `src-tauri/tauri.conf.json` share version 0.1.0. Release builds are produced by the GitHub workflow. Signed builds require platform signing credentials configured as GitHub secrets; none are stored in this repository.

## Safety model

The UI can request deletion only for files selected by the local user. Rust resolves each requested path, ensures it remains below the latest scan root, rejects recognized system locations, then uses the native trash/recycle-bin mechanism. There is no shell execution and no permanent-delete command.

## Hatchable integration

The backend base is `https://spacepilot.hatchable.site`. Future account, licensing, and `/api/ai/explain` calls should be configured with an environment value and secure credential storage. Core storage features do not require an account or network access.
# AI Copilot development provider

SpacePilot 0.5.0 uses a provider abstraction behind the native Rust boundary. The development OpenAI provider uses the Responses API with strict structured outputs and defaults to `gpt-5-mini`, selected for short, well-defined, cost-sensitive storage explanations. Override it with `SPACEPILOT_OPENAI_MODEL` when testing compatible models.

Set `OPENAI_API_KEY` in the environment that launches `npm run tauri:dev`. The key is read only by Rust, is not stored in SpacePilot settings, is never sent to React, and must not be embedded in distributed builds. Production must replace this development credential flow with SpacePilot backend authentication and provider proxying.

Cloud requests happen only after an explicit question. Requests use `store: false`, a 25-second timeout, a 16 KiB maximum sanitized context, a 1,000-character question limit, and a 1,200-token output limit. Only aggregate disk/scan/category/duplicate/Timeline/Developer Storage facts and opaque IDs are sent. File contents, hashes, raw paths, usernames, filenames, project names, and raw scan indexes are excluded.

## Private beta testing

SpacePilot Windows private beta builds are distributed as MSI artifacts from GitHub Actions. They may be unsigned, so Windows may show "Unknown Publisher" or Microsoft Defender SmartScreen warnings. Do not disable Windows security features; only install beta builds if you trust the source and understand this limitation.

Tester flow:

1. Download the MSI from the approved GitHub Actions artifact.
2. Install SpacePilot.
3. Launch SpacePilot.
4. Choose a safe drive or folder to scan.
5. Review Dashboard, Storage, Large Files, Duplicates, Cleanup, Space Rescue, Timeline, and Developer Storage results.
6. Optional: open Settings -> AI Provider and connect your own OpenRouter or Groq key.
7. Report bugs with screenshots and reproduction steps.

Do not post API keys, passwords, private documents, personal information, or sensitive file paths in GitHub Issues or bug reports.

## Draft release notes: SpacePilot 0.6.1 Beta 1

SpacePilot is an early Windows beta for local-first storage analysis.

Features:

- local storage scanning
- Storage visualization
- Large Files
- verified Duplicates
- Cleanup review
- Space Rescue
- Storage Timeline
- Developer Storage Intelligence
- AI Copilot
- BYOK OpenRouter/Groq

Privacy:

- storage scanning is local
- BYOK API credentials are stored in OS secure credential storage
- cloud AI requires the selected provider
- File Inspector content is only sent after explicit user approval

Known limitations:

- beta software
- Windows-focused build
- installer may be unsigned
- provider limits and availability are controlled by providers
- hosted SpacePilot services may not yet be available
- Google OAuth may not yet be configured
