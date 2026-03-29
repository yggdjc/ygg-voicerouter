//! Five-bar waveform widget driven by 3-level audio input.
//!
//! Uses a GtkDrawingArea with cairo rendering. The bars smoothly interpolate
//! between target heights using attack/release smoothing and random jitter.

use std::cell::Cell;
use std::f64::consts::PI;

use gtk4::prelude::*;
use gtk4::DrawingArea;

const BAR_COUNT: usize = 5;
const BAR_WIDTH: f64 = 4.0;
const BAR_GAP: f64 = 4.0;
const BAR_RADIUS: f64 = 2.0;
const MAX_HEIGHT: f64 = 32.0;
const MIN_HEIGHT: f64 = 4.0;

/// Relative height weights per bar: center-tall profile.
const WEIGHTS: [f64; BAR_COUNT] = [0.5, 0.8, 1.0, 0.75, 0.55];

/// Level-to-amplitude: fraction of weighted max height.
const LEVEL_AMP: [f64; 3] = [0.12, 0.50, 1.00];

const ATTACK: f64 = 0.40;
const RELEASE: f64 = 0.15;
const JITTER: f64 = 0.04;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaveMode {
    Level(u8),
    Pulse,
    Frozen,
    Off,
}

/// RGBA color for waveform bars.
#[derive(Debug, Clone, Copy)]
pub struct BarColor {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl BarColor {
    /// Blue-500: recording state.
    pub const RECORDING: Self = Self { r: 0.231, g: 0.510, b: 0.965, a: 0.90 };
    /// Purple-600: thinking/processing state.
    pub const THINKING: Self = Self { r: 0.576, g: 0.200, b: 0.918, a: 0.80 };
    /// Emerald-500: speaking/TTS state.
    pub const SPEAKING: Self = Self { r: 0.063, g: 0.725, b: 0.506, a: 0.85 };
    /// Default white (fallback).
    pub const DEFAULT: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 0.9 };
}

pub struct WaveformState {
    pub mode: Cell<WaveMode>,
    pub color: Cell<BarColor>,
    heights: [Cell<f64>; BAR_COUNT],
    tick: Cell<u64>,
}

impl WaveformState {
    pub fn new() -> Self {
        Self {
            mode: Cell::new(WaveMode::Off),
            color: Cell::new(BarColor::DEFAULT),
            heights: std::array::from_fn(|_| Cell::new(MIN_HEIGHT)),
            tick: Cell::new(0),
        }
    }

    pub fn tick(&self) {
        let t = self.tick.get();
        self.tick.set(t.wrapping_add(1));

        let mode = self.mode.get();
        for (i, weight) in WEIGHTS.iter().enumerate() {
            let target = match mode {
                WaveMode::Level(lvl) => {
                    let amp = LEVEL_AMP[lvl.min(2) as usize];
                    weight * amp * MAX_HEIGHT
                }
                WaveMode::Pulse => {
                    let phase = (t as f64 + i as f64 * 3.0) * 2.0 * PI / 36.0;
                    let pulse = 0.2 + 0.4 * (0.5 + 0.5 * phase.sin());
                    weight * pulse * MAX_HEIGHT
                }
                WaveMode::Frozen => weight * 0.20 * MAX_HEIGHT,
                WaveMode::Off => MIN_HEIGHT,
            };

            let current = self.heights[i].get();
            let factor = if target > current { ATTACK } else { RELEASE };
            let mut next = current + (target - current) * factor;

            if mode != WaveMode::Off && mode != WaveMode::Frozen {
                let jitter = (pseudo_random(t, i) * 2.0 - 1.0) * JITTER * next;
                next += jitter;
            }

            self.heights[i].set(next.clamp(MIN_HEIGHT, MAX_HEIGHT));
        }
    }

    pub fn draw(&self, cr: &gtk4::cairo::Context, _width: f64, height: f64) {
        let y_center = height / 2.0;
        let c = self.color.get();
        cr.set_source_rgba(c.r, c.g, c.b, c.a);

        for (i, _) in WEIGHTS.iter().enumerate() {
            let bar_h = self.heights[i].get();
            let x = i as f64 * (BAR_WIDTH + BAR_GAP);
            let y = y_center - bar_h / 2.0;
            rounded_rect(cr, x, y, BAR_WIDTH, bar_h, BAR_RADIUS);
            let _ = cr.fill();
        }
    }
}

fn rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

fn pseudo_random(tick: u64, bar: usize) -> f64 {
    let seed = tick.wrapping_mul(2654435761).wrapping_add(bar as u64);
    let hash = seed.wrapping_mul(0x517cc1b727220a95) >> 33;
    (hash % 1000) as f64 / 1000.0
}

pub fn create_waveform_widget(state: &std::rc::Rc<WaveformState>) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_size_request(44, 32);
    area.set_widget_name("waveform");

    let state_clone = state.clone();
    area.set_draw_func(move |_area, cr, width, height| {
        state_clone.draw(cr, width as f64, height as f64);
    });

    area
}
