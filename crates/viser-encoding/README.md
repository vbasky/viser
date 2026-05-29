# viser-encoding

Shared encoding configuration, codec-specific preset mapping, and temp directory cleanup.

## Key Types

- `Config` — common encoding parameters (resolutions, CRF values, codecs, preset, subsample, parallelism)

## Key Functions

- `preset_for_codec(codec, preset)` — maps generic preset names (`"veryfast"`) to codec-specific values (`"10"` for SVT-AV1)
- `clean_stale_temp_dirs(max_age)` — removes orphaned temp directories from crash recovery
