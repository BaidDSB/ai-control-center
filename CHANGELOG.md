# Changelog

All notable changes to **AI Control Center** are documented here.

## [v1.2.0] - 2026-11-28

### Added
- **Centralized Secondary Node & Auto-RPC Fallback:** Configure a LAN secondary node once in Setup (host, RPC port, peer port, live TCP probe, and `Auto` / `Strict` fallback policy). Profiles automatically inherit the secondary RPC endpoint and drop `--rpc` cleanly when the node is offline under `Auto`.
- **`llama-swap` Peers, Concurrent Groups & `checkEndpoint`:** Visual editors for `peers:` (with one-click **+ From Secondary Node**), `groups:` (`swap`, `exclusive`, `persistent`), per-profile `checkEndpoint`, and automatic same-IP **RPC + Peer host collision warnings** in the sidebar, YAML drawer, and launch log.
- **Live Telemetry (Dedicated VRAM + Shared GPU Memory + `/running`):** Background poller surfaces live Dedicated VRAM (`nvidia-smi` with WDDM/DRM fallback), live Shared GPU memory (Win32 PDH / Linux GTT), System RAM hover details, active `llama-swap` models, **Soft Unload** (`/api/models/unload` with `/unload` fallback), and **Swap /ui** quick link.
- **Pre-Launch GGUF VRAM Estimator:** Extended GGUF header probe reads attention KV head metadata to estimate weights, KV cache, buffer overhead, and RAM offload against your local GPU before launch.
- **System Tray, Profile Search & Portable Export/Import:** Opt-in Windows **Close to System Tray** (`Show`, `Open Browser`, `Soft Unload`, `Stop All`, `Quit`), instant sidebar search across families/profiles, and path-stripped `.toml` profile export/import with automatic GGUF re-linking.
- **Two-Way Simple <-> Advanced CLI Sync:** Edits in Simple Mode update Advanced Manual Flags live, and edits in Advanced Mode parse cleanly back into Simple controls (routing unrecognized flags to **Extra Args**) while preserving Auto/Strict RPC handling.

### Changed
- Modularized `src/config/`, `src/runtime/`, and `src/ui/` into focused submodules with 39 unit tests.
- Hardened cross-platform `rfd` file dialogs against blank/missing directories on Linux and added strict `cargo clippy --all-targets -- -D warnings` gates for Core and Plus in GitHub Actions CI.
- Enforced consistent `LF` line endings via `.gitattributes` and derived Core/Plus runtime version labels directly from `Cargo.toml`.

## [v1.1.0]

### Added
- Tracked PID + start-time process ownership (`running_state.toml`) for safe cross-platform stops and restarts.
- Live `llama-swap.yaml` synchronization on config save and expanded multi-engine routing controls.

## [v1.0.0]

### Added
- Initial public release of **AI Control Center Core** and **Plus** for Windows and Linux (`eframe`/`egui`).
