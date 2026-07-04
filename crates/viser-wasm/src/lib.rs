//! WASM bindings for viser no-reference quality metrics.
//!
//! Exposes pure-Rust frame scoring (sharpness, blockiness, noise, NIQE, BRISQUE)
//! for browser-side metric overlays in the comparison player. Frame decode remains
//! a host responsibility — pass gray8 luma buffers from a WebCodecs or canvas path.

use viser_quality::brisque;
use viser_quality::niqe;
use viser_quality::noref::{blockiness, noise_sigma, variance_of_laplacian};
use wasm_bindgen::prelude::*;

/// Variance-of-Laplacian sharpness for a gray8 frame (`width` × `height` bytes).
#[wasm_bindgen]
pub fn sharpness(gray: &[u8], width: usize, height: usize) -> f64 {
    variance_of_laplacian(gray, width, height)
}

/// 8×8 blockiness signal (lower is better).
#[wasm_bindgen]
pub fn blockiness_score(gray: &[u8], width: usize, height: usize) -> f64 {
    blockiness(gray, width, height)
}

/// Immerkær noise standard-deviation estimate (lower is cleaner).
#[wasm_bindgen]
pub fn noise_score(gray: &[u8], width: usize, height: usize) -> f64 {
    noise_sigma(gray, width, height)
}

/// NIQE score for one gray8 frame (lower is better).
#[wasm_bindgen]
pub fn niqe_score(gray: &[u8], width: usize, height: usize) -> f64 {
    niqe::score_frame(gray, width, height)
}

/// BRISQUE score for one gray8 frame (lower is better).
#[wasm_bindgen]
pub fn brisque_score(gray: &[u8], width: usize, height: usize) -> f64 {
    brisque::score_frame(gray, width, height)
}

/// All five no-reference signals for one gray8 frame.
#[wasm_bindgen]
pub struct NoRefFrameScores {
    sharpness: f64,
    blockiness: f64,
    noise: f64,
    niqe: f64,
    brisque: f64,
}

#[wasm_bindgen]
impl NoRefFrameScores {
    #[wasm_bindgen(getter)]
    pub fn sharpness(&self) -> f64 {
        self.sharpness
    }

    #[wasm_bindgen(getter)]
    pub fn blockiness(&self) -> f64 {
        self.blockiness
    }

    #[wasm_bindgen(getter)]
    pub fn noise(&self) -> f64 {
        self.noise
    }

    #[wasm_bindgen(getter)]
    pub fn niqe(&self) -> f64 {
        self.niqe
    }

    #[wasm_bindgen(getter)]
    pub fn brisque(&self) -> f64 {
        self.brisque
    }
}

/// Score all no-reference signals on one gray8 frame in a single call.
#[wasm_bindgen]
pub fn score_noref_frame(gray: &[u8], width: usize, height: usize) -> NoRefFrameScores {
    NoRefFrameScores {
        sharpness: variance_of_laplacian(gray, width, height),
        blockiness: blockiness(gray, width, height),
        noise: noise_sigma(gray, width, height),
        niqe: niqe::score_frame(gray, width, height),
        brisque: brisque::score_frame(gray, width, height),
    }
}
