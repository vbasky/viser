# viser System Design

## Architecture Overview

```mermaid
graph TB
    subgraph CLI["viser-cli (Application Layer)"]
        CMD[CLI Commands<br/>encode · inspect · quality<br/>per-title · per-shot · per-segment<br/>context-aware · compare]
    end

    subgraph Pipelines["Optimization Pipelines"]
        PT[viser-pertitle<br/>Per-Title Analysis]
        PS[viser-pershot<br/>Per-Shot + Trellis]
        SEG[viser-persegment<br/>Segment CRF Adaptation]
        CA[viser-contextaware<br/>Device-Specific Ladders]
    end

    subgraph Core["Core Analysis"]
        HULL[viser-hull<br/>Convex Hull + BD-Rate]
        LADDER[viser-ladder<br/>Rung Selection]
        SHOT[viser-shot<br/>Scene Detection]
        CX[viser-complexity<br/>Spatial/Temporal Analysis]
    end

    subgraph Foundation["Foundation Layer"]
        FF[viser-ffmpeg<br/>Encode · Probe · Cache]
        QM[viser-quality<br/>VMAF · PSNR · SSIM]
        ENC[viser-encoding<br/>Config · Presets · Progress]
        CP[viser-checkpoint<br/>Resumable State]
    end

    subgraph Output["Output & Visualization"]
        CMP[viser-compare<br/>Browser Comparison Player]
        CHT[viser-chart<br/>R-D Curve Charts]
    end

    CMD --> PT & PS & SEG & CA & CMP & CHT & FF & QM

    PT --> FF & QM & HULL & LADDER & ENC & CP
    PS --> PT & SHOT & FF & HULL & LADDER & ENC
    SEG --> FF & QM & CX
    CA --> PT & FF & HULL & LADDER & ENC

    LADDER --> FF & HULL

    classDef app fill:#4a90d9,stroke:#2c5f8a,color:#fff
    classDef pipeline fill:#e8744f,stroke:#b85a3e,color:#fff
    classDef core fill:#50b878,stroke:#3a8a5a,color:#fff
    classDef foundation fill:#9b7fc4,stroke:#7a60a0,color:#fff
    classDef output fill:#d4a843,stroke:#a88535,color:#fff

    class CMD app
    class PT,PS,SEG,CA pipeline
    class HULL,LADDER,SHOT,CX core
    class FF,QM,ENC,CP foundation
    class CMP,CHT output
```

## Data Flow: Per-Title Pipeline (Core)

```mermaid
flowchart LR
    A[Video File] --> B[viser-ffmpeg<br/>probe]
    B --> C[Trial Matrix<br/>res × codec × CRF]
    C --> D{Checkpoint<br/>exists?}
    D -- skip completed --> E
    D -- new trial --> E[Parallel Encode<br/>+ Quality Measure]
    E --> F[R-D Points<br/>bitrate, VMAF]
    F --> G[Convex Hull<br/>Pareto frontier]
    G --> H[Ladder Selection<br/>N optimal rungs]
    H --> I[Bitrate Ladder]

    style A fill:#f5f5f5,stroke:#999
    style I fill:#4a90d9,stroke:#2c5f8a,color:#fff
```

## Data Flow: Per-Shot Pipeline

```mermaid
flowchart LR
    A[Video File] --> B[Shot Detection<br/>scdet filter]
    B --> C[Shot Segments]
    C --> D[Per-Title Analysis<br/>per shot]
    D --> E[Shot R-D Hulls]
    E --> F[Trellis Optimization<br/>Lagrangian λ search]
    F --> G[Per-Shot<br/>Assignments]

    style A fill:#f5f5f5,stroke:#999
    style G fill:#e8744f,stroke:#b85a3e,color:#fff
```

## Data Flow: Segment-Level Adaptation

```mermaid
flowchart LR
    A[Video File] --> B[Complexity Analysis<br/>spatial + temporal + DCT]
    B --> C[Initial CRF Map<br/>complexity → CRF]
    C --> D[Binary Search<br/>per segment]
    D --> E{VMAF within<br/>tolerance?}
    E -- no --> D
    E -- yes --> F[Segment CRF<br/>Assignments]

    style A fill:#f5f5f5,stroke:#999
    style F fill:#50b878,stroke:#3a8a5a,color:#fff
```

## Data Flow: Context-Aware Encoding

```mermaid
flowchart LR
    A[Video File] --> B[Device Profiles]
    B --> C[Mobile<br/>≤720p, AV1+H.264<br/>150-3000 kbps]
    B --> D[Desktop<br/>≤1080p, All codecs<br/>200-8000 kbps]
    B --> E[TV 1080p<br/>≤1080p, AV1+H.265+H.264<br/>200-12000 kbps]
    B --> F[TV 4K<br/>≤2160p, AV1+H.265<br/>1000-25000 kbps]
    C & D & E & F --> G[Per-Title Analysis<br/>per device]
    G --> H[Device-Specific<br/>Ladders]

    style A fill:#f5f5f5,stroke:#999
    style H fill:#d4a843,stroke:#a88535,color:#fff
```

## Crate Dependency Graph

```mermaid
graph BT
    FF[viser-ffmpeg]
    QM[viser-quality]
    HULL[viser-hull]
    SHOT[viser-shot]
    CX[viser-complexity]
    ENC[viser-encoding]
    CP[viser-checkpoint]
    CMP[viser-compare]
    CHT[viser-chart]
    LADDER[viser-ladder]
    PT[viser-pertitle]
    PS[viser-pershot]
    SEG[viser-persegment]
    CA[viser-contextaware]
    CLI[viser-cli]

    LADDER --> FF & HULL
    PT --> FF & QM & HULL & LADDER & ENC & CP
    PS --> FF & HULL & LADDER & ENC & PT & SHOT
    SEG --> FF & QM & CX
    CA --> FF & HULL & LADDER & ENC & PT
    CLI --> FF & QM & HULL & LADDER & SHOT & CX & ENC & CP & CMP & CHT & PT & PS & SEG & CA

    classDef foundation fill:#9b7fc4,stroke:#7a60a0,color:#fff
    classDef core fill:#50b878,stroke:#3a8a5a,color:#fff
    classDef pipeline fill:#e8744f,stroke:#b85a3e,color:#fff
    classDef app fill:#4a90d9,stroke:#2c5f8a,color:#fff

    class FF,QM,ENC,CP,CMP,CHT foundation
    class HULL,LADDER,SHOT,CX core
    class PT,PS,SEG,CA pipeline
    class CLI app
```

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **VMAF as primary metric** | Perceptual quality correlates better with human perception than PSNR/SSIM |
| **Convex hull optimization** | Pareto-optimal R-D frontier eliminates dominated encoding points |
| **Trellis (Lagrangian) allocation** | Constant-slope bit distribution maximizes aggregate quality across shots |
| **Semaphore-gated parallelism** | Bounds concurrent encodes to `num_cpus/2` to avoid thrashing |
| **SHA256 checkpoint hashing** | Automatic invalidation when config changes; safe resume otherwise |
| **Codec-agnostic pipeline** | Same optimization framework works for H.264, H.265, and AV1 |
| **Layered crate architecture** | Each crate has single responsibility; pipelines compose foundation crates |
