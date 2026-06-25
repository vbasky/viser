# Chunked Encoding

Chunked encoding is a parallelization strategy for per-shot encoding. Instead
of encoding an entire video sequentially, it splits the video into chunks that
can be processed independently across multiple workers, dramatically reducing
wall-clock encoding time.

This document describes how chunked encoding works, how it integrates with
per-shot optimization, and how it fits into a production transcoding workflow.

**Status:** Documented for future implementation. Not yet built in viser.

## The Problem

Per-shot encoding produces excellent quality-per-bit, but the encoding step is
sequential by default - each shot is encoded one after another. For a 2-hour
feature film with 2,000 shots across a 6-rung bitrate ladder, that's 12,000
individual encodes. Even with fast presets, this can take hours.

The solution: **parallelize across chunks of shots**.

## Netflix's Approach

Netflix evolved through three generations:

### Generation 1: Fixed Chunks (~2015)

Split video into fixed ~3-minute chunks with no content awareness. Each chunk
is encoded independently by a worker. Simple, but chunk boundaries often fall
mid-shot, causing quality discontinuities and reducing compression efficiency.

### Generation 2: Individual Shots (~2018)

Encode each shot independently. Maximizes per-shot optimization but creates
problems:

- **Encoder warmup**: x264/x265 lookahead needs ~20 frames to reach optimal
  rate control. For a 4-second shot (96 frames at 24fps), ~20% of frames get
  suboptimal compression.
- **Orchestration overhead**: A 1-hour episode has ~900 shots vs ~20 chunks.
  Two orders of magnitude more distributed tasks overwhelmed Netflix's
  messaging infrastructure.
- **Keyframe overhead**: Each independently-encoded shot starts with an IDR
  frame. Short action shots (0.5-1s) pay 4-8% overhead.

### Generation 3: Collated Chunks (~2019, Current)

Group an integer number of consecutive shots into **~3-minute chunks**:

```text
Video: [shot1][shot2][shot3][shot4][shot5][shot6][shot7][shot8][shot9]...

Chunk 1 (~3 min):        Chunk 2 (~3 min):        Chunk 3:
[shot1][shot2][shot3]    [shot4][shot5][shot6]    [shot7][shot8][shot9]
```

- **Shots** are the unit of quality optimization (each gets its own R-D
  parameters from Trellis optimization)
- **Chunks** are the unit of distributed work (each is encoded by one worker)
- Within a chunk, the worker encodes each shot with its assigned parameters,
  switching CRF/resolution at shot boundaries

This preserves per-shot quality optimization while keeping orchestration
manageable and encoder warmup negligible.

## How It Works

### Step 1: Analyze (Sequential)

Run the per-shot analysis pipeline to determine optimal parameters:

```
Source Video
       │
       ▼
┌────────────────┐
│ Shot Detection │  Identify shot boundaries
└───────┬────────┘
        │
        ▼
┌────────────────┐
│ Per-Shot Hull  │  Compute R-D hull per shot
│ Analysis       │  (can be parallelized per shot)
└───────┬────────┘
        │
        ▼
┌────────────────┐
│ Trellis        │  Assign (resolution, CRF) per shot per rung
│ Optimization   │
└───────┬────────┘
        │
        ▼
  Shot assignments ready
```

### Step 2: Collate Shots into Chunks

Group consecutive shots into chunks targeting ~3 minutes each:

```text
Constraints:
  - Chunk boundaries must align with shot boundaries
  - Target duration: 1-4 minutes (3 min ideal)
  - Integer number of shots per chunk
  - No shot split across chunks
```

Algorithm:

```text
target_duration = 180 seconds  (3 minutes)
chunks = []
current_chunk = []
current_duration = 0

for each shot in shots:
    if current_duration + shot.duration > target_duration * 1.5
       and current_duration > target_duration * 0.5:
        chunks.append(current_chunk)
        current_chunk = [shot]
        current_duration = shot.duration
    else:
        current_chunk.append(shot)
        current_duration += shot.duration

chunks.append(current_chunk)  // final chunk
```

### Step 3: Parallel Encode (Distributed)

Each chunk is encoded independently, producing one segment per bitrate rung:

```
                ┌─────────────────────────────┐
                │         Job Queue           │
                │                             │
                │  Chunk 1, Rung 1 (480p)     │
                │  Chunk 1, Rung 2 (720p)     │
                │  Chunk 1, Rung 3 (1080p)    │
                │  Chunk 2, Rung 1 (480p)     │
                │  Chunk 2, Rung 2 (720p)     │
                │  ...                        │
                └──────────────┬──────────────┘
                               │
              ┌────────────────┼────────────────┐
              v                v                v
        ┌──────────┐    ┌──────────┐    ┌──────────┐
        │ Worker 1 │    │ Worker 2 │    │ Worker 3 │
        │          │    │          │    │          │
        │ Encode   │    │ Encode   │    │ Encode   │
        │ chunk 1  │    │ chunk 1  │    │ chunk 2  │
        │ rung 1   │    │ rung 2   │    │ rung 1   │
        └──────────┘    └──────────┘    └──────────┘
```

Each worker:
1. Receives a chunk (list of shots with assigned parameters)
2. Extracts the chunk segment from the source video
3. Encodes the full chunk, switching CRF at shot boundaries using FFmpeg's
   zone or segment features
4. Uploads the encoded chunk segment

### Step 4: Assemble and Package

Concatenate chunk segments per rung into the final renditions:

```text
Rung 1 (480p):  [chunk1_480p] + [chunk2_480p] + [chunk3_480p] → 480p.mp4
Rung 2 (720p):  [chunk1_720p] + [chunk2_720p] + [chunk3_720p] → 720p.mp4
Rung 3 (1080p): [chunk1_1080p]+ [chunk2_1080p]+ [chunk3_1080p]→ 1080p.mp4
```

Then package into DASH/HLS manifests with segment boundaries aligned to
chunk/shot boundaries.

## Production Transcoding Workflow

A complete ingest-to-delivery pipeline using viser with chunked encoding:

```text
┌─────────┐   ┌───────────┐   ┌─────────────┐   ┌─────────────┐
│ Ingest  │──>│ Analyze   │──>│ Chunk &     │──>│ Encode      │
│ source  │   │ (viser)     │   │ Distribute  │   │ (workers)   │
└─────────┘   └───────────┘   └─────────────┘   └──────┬──────┘
                                                        │
              ┌───────────┐   ┌─────────────┐           │
              │ Deliver   │<──│ Package     │<──────────┘
              │ (CDN)     │   │ (DASH/HLS)  │
              └───────────┘   └─────────────┘
```

### 1. Ingest

- Receive source video (mezzanine quality, ProRes/DNxHR or lossless)
- Validate: probe format, resolution, frame rate, duration
- Store in object storage (S3, GCS)

### 2. Analyze (viser)

- Shot detection (scdet, ~real-time speed)
- Per-shot trial encodes at representative CRF values
- Convex hull computation per shot
- Trellis optimization: assign (resolution, CRF) per shot per ladder rung
- Output: shot list with encoding parameters per rung

### 3. Chunk and Distribute

- Collate shots into ~3-minute chunks
- For each chunk × rung, create an encoding job
- Submit jobs to work queue (SQS, Redis, RabbitMQ, etc.)

### 4. Encode (Parallel Workers)

- Workers pull jobs from the queue
- Each worker encodes one chunk at one rung
- Workers can be auto-scaled based on queue depth
- Output: encoded chunk segments stored in object storage

### 5. Package (DASH/HLS)

- Concatenate chunks per rung into final renditions
- Force IDR frames at chunk boundaries
- Generate DASH/HLS manifests
- Validate: check for A/V sync, segment alignment, quality thresholds

### 6. Deliver

- Push to CDN
- Manifests reference segments by URL
- ABR player selects appropriate rung based on bandwidth

## Parallelism Math

For a 2-hour feature film:

```
Duration:     7,200 seconds
Avg shot:     ~4 seconds → ~1,800 shots
Chunk target: 180 seconds → ~40 chunks
Ladder rungs: 6

Total jobs: 40 chunks × 6 rungs = 240 encoding jobs

Sequential time (1 worker):    ~240 × 3 min = 12 hours
With 20 workers:               ~12 jobs/worker = ~36 min
With 60 workers:               ~4 jobs/worker = ~12 min
```

Netflix processes ~40 chunks per title with ~20 workers, achieving encode times
comparable to the video duration (near real-time for the encode phase).

## Chunk Duration Sweet Spot

| Duration | Pros | Cons |
|----------|------|------|
| < 30s | Maximum parallelism | Encoder warmup dominates, high overhead |
| 30s - 1 min | Good parallelism | Some warmup impact on short chunks |
| **1 - 4 min** | **Ideal: negligible warmup, good parallelism** | **Recommended** |
| 5 - 10 min | Minimal overhead | Fewer parallel jobs, longer per-job time |
| > 10 min | Lowest overhead | Poor parallelism, approaches sequential |

The ~3-minute sweet spot balances:
- **Encoder efficiency**: warmup is < 0.5% of frames at 3 min
- **Parallelism**: 40 chunks for a feature film = 40x potential speedup
- **Fault tolerance**: if a worker fails, only ~3 min of work is lost
- **Orchestration**: manageable number of jobs (hundreds, not thousands)

## FFmpeg Implementation Notes

Within a chunk, switching encoding parameters at shot boundaries can be done
with FFmpeg's zone or forced keyframe features:

```bash
# Force keyframes at shot boundaries within the chunk
ffmpeg -i chunk.mp4 \
  -force_key_frames "2.5,5.1,8.3" \
  -c:v libx264 -crf 28 \
  -output.mp4

# Or use x264's zone feature for per-region CRF
ffmpeg -i chunk.mp4 \
  -c:v libx264 \
  -x264-params "zones=0,60,crf=24/61,125,crf=30/126,200,crf=28" \
  output.mp4
```

For SVT-AV1, per-frame QP can be controlled via a qpfile:

```
0 I 24    # frame 0: IDR, QP 24
60 I 30   # frame 60: IDR, QP 30 (shot boundary)
125 I 28  # frame 125: IDR, QP 28 (shot boundary)
```

## Further Reading

### Industry practice

- Netflix: [Optimized Shot-Based Encodes](https://netflixtechblog.com/optimized-shot-based-encodes-now-streaming-4b9464204830)
- Netflix: [Per-Title Encode Optimization](https://netflixtechblog.com/per-title-encode-optimization-7e99450b2588)
- Bitmovin: [Split and Stitch Encoding](https://bitmovin.com/blog/split-and-stitch-encoding/)
- viser Research: [Chunked Encoding Collation](../research/12-chunked-encoding-collation.md)

### Research papers

- Giladi et al., [Massively Parallel Open Source Encoding for Adaptive Streaming](https://journal.smpte.org/conferences/SMPTE%202018/21/), SMPTE 2018 — defines the chunk encode "joblet" and discusses distributed encoding without significant quality impact.
- Neugebauer, [Nagare Media Engine: A System for Cloud- and Edge-Native Network-based Multimedia Workflows](https://arxiv.org/abs/2509.24546), arXiv 2025 — NBMP-based cloud/edge workflow system covering task distribution and recovery.
- Li et al., [Performance Analysis and Modeling of Video Transcoding Using Heterogeneous Cloud Services](https://arxiv.org/abs/1809.06529), arXiv 2018 — performance modeling of cloud video transcoding across heterogeneous instances.
- Shu et al., [Predicting total time to compress a video corpus using online inference systems](https://arxiv.org/abs/2410.18260), IEEE VCIP 2024 — corpus-level transcoding cost prediction for cloud VOD pipelines.
- Durbha et al., [Leveraging Compression to Construct Transferable Bitrate Ladders](https://arxiv.org/abs/2512.12952), arXiv 2025 — ML-based construction of content-adaptive, per-shot bitrate ladders and convex-hull approximations.
