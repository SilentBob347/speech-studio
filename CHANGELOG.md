# Changelog

What changed, newest first. Dates are release dates; the app is versioned by its Windows
build. Every change a user can see is written here in the commit that makes it, and the
release notes on GitHub are taken from the release's section.

## Unreleased — 0.1.0

### Added

- **Two modes in one studio.** Speech (voicing text) and Listening (recognising speech and checking a voicing) switch from the top bar or with Ctrl+1 and Ctrl+2.
- **First run and the model manager.** The studio checks the graphics card, driver and runtime libraries, suggests a set of models for the card, downloads them in the background with pause and resume and checks every file against its SHA-256; models already on disk are taken from a folder.
- **Settings by section**: models, cloud, quality, network, language and sounds, agent (MCP), anonymous statistics, about.
- **Resource monitor**: graphics card load, memory, temperature and power, system memory.
- **Agent (MCP)** at `http://127.0.0.1:8797/mcp`: models, voices, recording, keys, proxy, links and the window.
