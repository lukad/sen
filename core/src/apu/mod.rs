mod dmc;
mod envelope;
mod noise;
mod pulse;
mod triangle;

use crate::apu::{
    dmc::Dmc,
    noise::Noise,
    pulse::{Pulse, SweepNegateMode},
    triangle::Triangle,
};

pub(crate) use crate::apu::dmc::{DmcDmaKind, DmcDmaRequest};

const CPU_HZ: f64 = 1_789_773.0;

const HIGH_PASS_37_HZ: f64 = 37.0;
const LOW_PASS_14_KHZ: f64 = 14_000.0;

pub(crate) const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

#[derive(Default)]
struct FrameEvents {
    quarter: bool,
    half: bool,
}

impl FrameEvents {}

struct NesAudioFilter {
    high_pass_37: HighPassFilter,
    low_pass_14k: LowPassFilter,
}

impl NesAudioFilter {
    fn new(sample_rate: f64) -> Self {
        Self {
            high_pass_37: HighPassFilter::new(sample_rate, HIGH_PASS_37_HZ),
            low_pass_14k: LowPassFilter::new(sample_rate, LOW_PASS_14_KHZ),
        }
    }

    fn apply(&mut self, sample: f32) -> f32 {
        let sample = self.high_pass_37.apply(sample);
        self.low_pass_14k.apply(sample)
    }
}

struct HighPassFilter {
    alpha: f32,
    previous_input: f32,
    previous_output: f32,
}

impl HighPassFilter {
    fn new(sample_rate: f64, cutoff_hz: f64) -> Self {
        let alpha = sample_rate / (sample_rate + 2.0 * std::f64::consts::PI * cutoff_hz);

        Self {
            alpha: alpha as f32,
            previous_input: 0.0,
            previous_output: 0.0,
        }
    }

    fn apply(&mut self, input: f32) -> f32 {
        let output = self.alpha * (self.previous_output + input - self.previous_input);
        self.previous_input = input;
        self.previous_output = output;
        output
    }
}

struct LowPassFilter {
    alpha: f32,
    previous_output: f32,
}

impl LowPassFilter {
    fn new(sample_rate: f64, cutoff_hz: f64) -> Self {
        let rc_factor = 2.0 * std::f64::consts::PI * cutoff_hz;
        let alpha = rc_factor / (sample_rate + rc_factor);

        Self {
            alpha: alpha as f32,
            previous_output: 0.0,
        }
    }

    fn apply(&mut self, input: f32) -> f32 {
        self.previous_output += self.alpha * (input - self.previous_output);
        self.previous_output
    }
}

pub(crate) struct Apu {
    pulse1: Pulse,
    pulse2: Pulse,
    triangle: Triangle,
    noise: Noise,
    dmc: Dmc,
    pulse_timer_phase: bool,
    frame_cycle: usize,
    five_step_mode: bool,
    irq_inhibit: bool,
    sample_rate: f64,
    sample_accumulator: f64,
    output_filter: NesAudioFilter,
}

impl Apu {
    pub(crate) fn new(sample_rate: f64) -> Self {
        Self {
            pulse1: Pulse::new(SweepNegateMode::OnesComplement),
            pulse2: Pulse::new(SweepNegateMode::TwosComplement),
            triangle: Triangle::new(),
            noise: Noise::new(),
            dmc: Dmc::new(),
            pulse_timer_phase: false,
            frame_cycle: 0,
            five_step_mode: false,
            irq_inhibit: false,
            sample_rate,
            sample_accumulator: 0.0,
            output_filter: NesAudioFilter::new(sample_rate),
        }
    }

    pub(crate) fn tick(&mut self, mut emit_sample: impl FnMut(f32)) {
        self.triangle.tick_timer();
        self.noise.tick_timer();
        self.dmc.tick_timer();

        self.pulse_timer_phase = !self.pulse_timer_phase;
        if self.pulse_timer_phase {
            self.pulse1.tick_timer();
            self.pulse2.tick_timer();
        }

        let events = self.tick_frame_counter();

        if events.quarter {
            self.clock_quarter_frame();
        }

        if events.half {
            self.clock_half_frame();
        }

        self.sample_accumulator += self.sample_rate;
        while self.sample_accumulator >= CPU_HZ {
            self.sample_accumulator -= CPU_HZ;
            emit_sample(self.output_filter.apply(self.mix()));
        }
    }

    pub(crate) fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4000 => self.pulse1.write_control(value),
            0x4001 => self.pulse1.write_sweep(value),
            0x4002 => self.pulse1.write_timer_low(value),
            0x4003 => self.pulse1.write_timer_high(value),
            0x4004 => self.pulse2.write_control(value),
            0x4005 => self.pulse2.write_sweep(value),
            0x4006 => self.pulse2.write_timer_low(value),
            0x4007 => self.pulse2.write_timer_high(value),
            0x4008 => self.triangle.write_linear_counter(value),
            0x400A => self.triangle.write_timer_low(value),
            0x400B => self.triangle.write_timer_high_and_length(value),
            0x400C => self.noise.write_control(value),
            0x400E => self.noise.write_period(value),
            0x400F => self.noise.write_length(value),
            0x4010 => self.dmc.write_flags_rate(value),
            0x4011 => self.dmc.write_direct_load(value),
            0x4012 => self.dmc.write_sample_address(value),
            0x4013 => self.dmc.write_sample_length(value),
            0x4015 => self.write_channel_enable(value),
            0x4017 => self.write_frame_counter(value),
            _ => (),
        }
    }

    pub(crate) fn read_status(&mut self) -> u8 {
        let mut value = 0;

        if self.pulse1.length_counter_active() {
            value |= 0x01;
        }

        if self.pulse2.length_counter_active() {
            value |= 0x02;
        }

        if self.triangle.length_counter_active() {
            value |= 0x04;
        }

        if self.noise.length_counter_active() {
            value |= 0x08;
        }

        if self.dmc.active() {
            value |= 0x10;
        }

        if self.dmc.interrupt_flag() {
            value |= 0x80;
        }

        value
    }

    fn write_channel_enable(&mut self, value: u8) {
        self.pulse1.set_enabled(value & 0x01 != 0);
        self.pulse2.set_enabled(value & 0x02 != 0);
        self.triangle.set_enabled(value & 0x04 != 0);
        self.noise.set_enabled(value & 0x08 != 0);
        self.dmc.set_enabled(value & 0x10 != 0);
    }

    fn write_frame_counter(&mut self, value: u8) {
        self.five_step_mode = value & 0x80 != 0;
        self.irq_inhibit = value & 0x40 != 0;
        self.frame_cycle = 0;

        if self.five_step_mode {
            self.clock_half_frame();
        }
    }

    fn tick_frame_counter(&mut self) -> FrameEvents {
        self.frame_cycle += 1;

        let mut events = FrameEvents::default();

        if self.five_step_mode {
            if matches!(self.frame_cycle, 7_457 | 14_913 | 22_371 | 37_281) {
                events.quarter = true;
            }

            if matches!(self.frame_cycle, 14_913 | 37_281) {
                events.half = true;
            }

            if self.frame_cycle >= 37_281 {
                self.frame_cycle = 0;
            }
        } else {
            if matches!(self.frame_cycle, 7_457 | 14_913 | 22_371 | 29_829) {
                events.quarter = true;
            }

            if matches!(self.frame_cycle, 14_913 | 29_829) {
                events.half = true;
            }

            if self.frame_cycle >= 29_829 {
                self.frame_cycle = 0;
            }
        }

        events
    }

    fn clock_quarter_frame(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
        self.triangle.clock_linear_counter();
        self.noise.clock_envelope();
    }

    fn clock_half_frame(&mut self) {
        self.pulse1.clock_length_counter();
        self.pulse2.clock_length_counter();
        self.pulse1.clock_sweep();
        self.pulse2.clock_sweep();
        self.triangle.clock_length_counter();
        self.noise.clock_length_counter();
    }

    fn mix(&self) -> f32 {
        let pulse_sum = self.pulse1.output() as f32 + self.pulse2.output() as f32;
        let pulse_out = if pulse_sum == 0.0 {
            0.0
        } else {
            95.88 / ((8128.0 / pulse_sum) + 100.0)
        };

        let triangle = self.triangle.output();
        let noise = self.noise.output() as f32;
        let dmc = self.dmc.output() as f32;

        let tnd_input = (triangle / 8227.0) + (noise / 12241.0) + (dmc / 22638.0);
        let tnd_out = if tnd_input == 0.0 {
            0.0
        } else {
            159.79 / ((1.0 / tnd_input) + 100.0)
        };

        pulse_out + tnd_out
    }

    pub(crate) fn take_dmc_dma_request(&mut self) -> Option<DmcDmaRequest> {
        self.dmc.take_dma_request()
    }

    pub(crate) fn finish_dmc_dma(&mut self, value: u8) {
        self.dmc.load_sample_buffer(value);
    }

    pub(crate) fn irq_asserted(&self) -> bool {
        self.dmc.interrupt_flag()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enable_audible_pulse1(apu: &mut Apu) {
        apu.write_register(0x4000, 0b0101_1111); // 25% duty, length halt, constant volume 15.
        apu.write_register(0x4002, 0xFF);
        apu.write_register(0x4015, 0x01);
        apu.write_register(0x4003, 0x00);
    }

    #[test]
    fn status_reports_pulse1_length_counter_when_enabled_and_loaded() {
        let mut apu = Apu::new(44_100.0);

        apu.write_register(0x4015, 0x01);
        apu.write_register(0x4003, 0x00);

        assert_eq!(apu.read_status() & 0x01, 0x01);
    }

    #[test]
    fn disabling_pulse1_clears_status_bit() {
        let mut apu = Apu::new(44_100.0);

        apu.write_register(0x4015, 0x01);
        apu.write_register(0x4003, 0x00);
        apu.write_register(0x4015, 0x00);

        assert_eq!(apu.read_status() & 0x01, 0x00);
    }

    #[test]
    fn pulse1_generates_nonzero_samples_when_enabled() {
        let mut apu = Apu::new(44_100.0);
        enable_audible_pulse1(&mut apu);

        let mut samples = Vec::new();

        for _ in 0..1_000 {
            apu.tick(|sample| samples.push(sample));
        }

        assert!(!samples.is_empty());
        assert!(samples.into_iter().any(|sample| sample > 0.0));
    }

    #[test]
    fn nes_output_filter_removes_dc_offset() {
        let mut filter = NesAudioFilter::new(44_100.0);
        let mut sample = 0.0;

        for _ in 0..44_100 {
            sample = filter.apply(0.5);
        }

        assert!(sample.abs() < 0.001);
    }

    #[test]
    fn nes_output_filter_preserves_bass_range() {
        let sample_rate = 44_100.0;
        let mut filter = NesAudioFilter::new(sample_rate);
        let mut input_sum = 0.0;
        let mut output_sum = 0.0;
        let mut count = 0.0;

        for i in 0..44_100 {
            let phase = 2.0 * std::f32::consts::PI * 110.0 * (i as f32 / sample_rate as f32);
            let input = phase.sin();
            let output = filter.apply(input);

            if i >= 4_410 {
                input_sum += input * input;
                output_sum += output * output;
                count += 1.0;
            }
        }

        let input_rms = f32::sqrt(input_sum / count);
        let output_rms = f32::sqrt(output_sum / count);

        assert!(output_rms / input_rms > 0.85);
    }

    #[test]
    fn noise_status_bit_reports_loaded_length_counter() {
        let mut apu = Apu::new(44_100.0);

        apu.write_register(0x4015, 0x08);
        apu.write_register(0x400F, 0x00);

        assert_eq!(apu.read_status() & 0x08, 0x08);
    }

    #[test]
    fn disabling_noise_clears_status_bit() {
        let mut apu = Apu::new(44_100.0);

        apu.write_register(0x4015, 0x08);
        apu.write_register(0x400F, 0x00);
        apu.write_register(0x4015, 0x00);

        assert_eq!(apu.read_status() & 0x08, 0x00);
    }
}
