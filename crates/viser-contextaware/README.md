# viser-contextaware

Device-specific ladder generation — per-title analysis tuned for each device class (Mobile, Desktop, TV, TV 4K) with resolution caps, codec preferences, and VMAF model selection.

## Key Types

- `Config` — device profiles, CRF values, preset, subsample, parallelism
- `Profile` / `DeviceClass` — encoding constraints per device class
- `Result` — per-device hulls and ladders

## Key Functions

- `analyze(source, cfg, progress_tx)` — runs per-title analysis for each device profile
- `all_profiles()` / `mobile_profile()` / `desktop_profile()` / `tv_profile()` / `tv_4k_profile()` — factory functions for standard profiles
