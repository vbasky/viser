# viser-checkpoint

Checkpoint/resume support for long-running analyses — persists completed encoding trials so multi-hour analyses survive crashes.

## Key Types

- `Checkpoint` — manages incremental trial persistence (load/save/query)

## Key Functions

- `config_hash(source, resolutions, codecs, crf_values, preset)` — deterministic SHA-256 of encoding config for invalidation
- `default_path(source)` — default checkpoint file path for a given source
