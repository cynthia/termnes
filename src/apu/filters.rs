/// First-order high-pass filter (removes DC offset / low-frequency rumble).
#[derive(Clone)]
pub struct HighPassFilter {
    b: f32,
    prev_input: f32,
    prev_output: f32,
}

impl HighPassFilter {
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let b = (-2.0 * std::f32::consts::PI * cutoff_hz / sample_rate).exp();
        Self {
            b,
            prev_input: 0.0,
            prev_output: 0.0,
        }
    }

    pub fn apply(&mut self, x: f32) -> f32 {
        let y = self.b * self.prev_output + x - self.prev_input;
        self.prev_input = x;
        self.prev_output = y;
        y
    }
}

/// First-order low-pass filter (anti-aliasing).
#[derive(Clone)]
pub struct LowPassFilter {
    alpha: f32,
    prev_output: f32,
}

impl LowPassFilter {
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let alpha = 1.0 - (-2.0 * std::f32::consts::PI * cutoff_hz / sample_rate).exp();
        Self {
            alpha,
            prev_output: 0.0,
        }
    }

    pub fn apply(&mut self, x: f32) -> f32 {
        self.prev_output += self.alpha * (x - self.prev_output);
        self.prev_output
    }
}
