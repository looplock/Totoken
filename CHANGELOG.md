# Changelog

All notable changes to Totoken are documented in this file.

## v0.1.0 - 2026-05-04

Initial public release.

### Desktop

#### Added

- Local AI coding tool activity inspection for Claude Code, Codex, Cursor, OpenCode, Kilo Code, and Kiro.
- Dashboards for token totals, estimated cost, scan status, recent activity, model usage, source distribution, and activity heatmaps.
- Session and message browsers for reviewing normalized local AI coding history.
- Model catalog sync from OpenRouter for model metadata, context windows, capabilities, and pricing used in local estimates.
- Scan records, scheduled scan settings, app data maintenance tools, backups, cache cleanup, database vacuum, and index rebuilds.
- English and Simplified Chinese interface support.

#### Release Assets

- Windows x64: MSI installer
- macOS Intel: DMG
- macOS Apple Silicon: DMG
- Linux x64: DEB and AppImage

#### Notes

- Totoken stores application data locally under `~/.totoken/`.
- Totoken focuses on local usage visibility. It does not run a model gateway, proxy provider traffic, or manage external API credentials.
- Model catalog data is provided by OpenRouter as a third-party metadata source and is used only for display and local cost estimation.
