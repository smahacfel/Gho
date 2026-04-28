#![allow(arithmetic_side_effects)]
#![allow(clippy::arithmetic_side_effects)]
//! E.C.T.O. (Early Chrono-Trade Observer)
//!
//! Ultra-light, deterministic analyzer for the 0-7s genesis window.
//! Produces a pure signal describing early market genetics (no decisions).

use std::collections::VecDeque;

use bitflags::bitflags;
use solana_sdk::pubkey::Pubkey;

/// Default ring buffer capacity (events)
pub const DEFAULT_ECTO_BUFFER_CAPACITY: usize = 50;

/// ECTO observation window in milliseconds (0-7s)
pub const ECTO_MAX_WINDOW_MS: u64 = 7_000;

/// ECTO runtime configuration
#[derive(Debug, Clone, Copy)]
pub struct EctoConfig {
    /// Circular buffer capacity
    pub buffer_capacity: usize,
    /// Threshold for sniper classification (SOL)
    pub sniper_threshold_sol: f64,
    /// Threshold for retail classification (SOL)
    pub retail_threshold_sol: f64,
    /// Minimum window length to emit a signal
    pub min_window_ms_emit: u64,
    /// Minimum confidence to emit a signal
    pub min_confidence_emit: f64,
    /// Sniper wall threshold (count in window)
    pub sniper_wall_count_threshold: usize,
    /// Sniper wall threshold (consecutive buys)
    pub sniper_wall_streak_threshold: usize,
    /// Retail swarm threshold (count in window)
    pub retail_swarm_count_threshold: usize,
    /// Buy streak threshold (consecutive buys)
    pub buy_streak_threshold: usize,
    /// Minimum events required for full confidence
    pub min_events_for_confidence: usize,
}

impl Default for EctoConfig {
    fn default() -> Self {
        Self {
            buffer_capacity: DEFAULT_ECTO_BUFFER_CAPACITY,
            sniper_threshold_sol: 1.0,
            retail_threshold_sol: 0.2,
            min_window_ms_emit: 1_500,
            min_confidence_emit: 0.3,
            sniper_wall_count_threshold: 3,
            sniper_wall_streak_threshold: 3,
            retail_swarm_count_threshold: 6,
            buy_streak_threshold: 4,
            min_events_for_confidence: 6,
        }
    }
}

/// Emitted ECTO signal (facts only, no decisions)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EctoSignal {
    /// bias ∈ [-1.0, 1.0]
    pub bias: f64,
    /// normalized score ∈ [0.0, 1.0]
    pub score: f64,
    /// confidence ∈ [0.0, 1.0]
    pub confidence: f64,
    /// genetic flags
    pub flags: EctoFlags,
    /// observed window length in ms
    pub window_ms: u64,
    /// verdict classification
    pub verdict: EctoVerdict,
}

/// ECTO verdict for hard-kill gating
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EctoVerdict {
    Rug,
    Neutral,
    Bullish,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct EctoFlags: u32 {
        const DEV_SOLD     = 0b0001;
        const SNIPER_WALL  = 0b0010;
        const RETAIL_SWARM = 0b0100;
        const BUY_STREAK   = 0b1000;
    }
}

#[derive(Debug, Clone, Copy)]
struct EctoEvent {
    is_buy: bool,
    is_dev: bool,
    is_sniper: bool,
    is_retail: bool,
    timestamp_ms: u64,
}

/// Stateful ECTO analyzer
pub struct EctoState {
    config: EctoConfig,
    dev_pubkey: Option<Pubkey>,
    events: VecDeque<EctoEvent>,
    first_ts_ms: Option<u64>,
    last_ts_ms: Option<u64>,
    dev_sell_count: usize,
    sniper_buy_count: usize,
    retail_buy_count: usize,
    buy_streak: usize,
    sniper_buy_streak: usize,
    last_event_buy: bool,
    last_event_sniper_buy: bool,
    last_signal: Option<EctoSignal>,
}

impl EctoState {
    pub fn new(config: EctoConfig, dev_pubkey: Option<Pubkey>) -> Self {
        let capacity = config.buffer_capacity.max(1);
        Self {
            config,
            dev_pubkey,
            events: VecDeque::with_capacity(capacity),
            first_ts_ms: None,
            last_ts_ms: None,
            dev_sell_count: 0,
            sniper_buy_count: 0,
            retail_buy_count: 0,
            buy_streak: 0,
            sniper_buy_streak: 0,
            last_event_buy: false,
            last_event_sniper_buy: false,
            last_signal: None,
        }
    }

    pub fn dev_pubkey(&self) -> Option<Pubkey> {
        self.dev_pubkey
    }

    pub fn set_dev_pubkey_once(&mut self, dev_pubkey: Pubkey) -> bool {
        if self.dev_pubkey.is_none() {
            self.dev_pubkey = Some(dev_pubkey);
            return true;
        }
        false
    }

    /// Update using signer info (dev determined by stored pubkey)
    pub fn update_with_signer(
        &mut self,
        signer: Pubkey,
        is_buy: bool,
        amount_sol: f64,
        timestamp_ms: u64,
    ) -> bool {
        let is_dev = self.dev_pubkey.map(|pk| pk == signer).unwrap_or(false);
        self.update(signer, is_buy, amount_sol, timestamp_ms, is_dev)
    }

    /// Update with explicit dev flag.
    /// Returns true if the event was accepted into the window.
    pub fn update(
        &mut self,
        signer: Pubkey,
        is_buy: bool,
        amount_sol: f64,
        timestamp_ms: u64,
        mut is_dev: bool,
    ) -> bool {
        if is_dev {
            let _ = self.set_dev_pubkey_once(signer);
        } else if let Some(dev) = self.dev_pubkey {
            is_dev = dev == signer;
        }

        if self.first_ts_ms.is_none() {
            self.first_ts_ms = Some(timestamp_ms);
        }

        let first_ts = self.first_ts_ms.unwrap_or(timestamp_ms);
        let age_ms = timestamp_ms.saturating_sub(first_ts);
        if age_ms > ECTO_MAX_WINDOW_MS {
            return false;
        }

        let is_sniper = is_buy && amount_sol >= self.config.sniper_threshold_sol;
        let is_retail = is_buy && amount_sol < self.config.retail_threshold_sol;

        if self.events.len() >= self.events.capacity() {
            if let Some(evicted) = self.events.pop_front() {
                if evicted.is_dev && !evicted.is_buy {
                    self.dev_sell_count = self.dev_sell_count.saturating_sub(1);
                }
                if evicted.is_sniper {
                    self.sniper_buy_count = self.sniper_buy_count.saturating_sub(1);
                }
                if evicted.is_retail {
                    self.retail_buy_count = self.retail_buy_count.saturating_sub(1);
                }
            }
        }

        self.events.push_back(EctoEvent {
            is_buy,
            is_dev,
            is_sniper,
            is_retail,
            timestamp_ms,
        });

        if is_dev && !is_buy {
            self.dev_sell_count = self.dev_sell_count.saturating_add(1);
        }
        if is_sniper {
            self.sniper_buy_count = self.sniper_buy_count.saturating_add(1);
        }
        if is_retail {
            self.retail_buy_count = self.retail_buy_count.saturating_add(1);
        }

        if is_buy {
            if self.last_event_buy {
                self.buy_streak = self.buy_streak.saturating_add(1);
            } else {
                self.buy_streak = 1;
            }
        } else {
            self.buy_streak = 0;
        }
        self.last_event_buy = is_buy;

        if is_buy && is_sniper {
            if self.last_event_sniper_buy {
                self.sniper_buy_streak = self.sniper_buy_streak.saturating_add(1);
            } else {
                self.sniper_buy_streak = 1;
            }
        } else {
            self.sniper_buy_streak = 0;
        }
        self.last_event_sniper_buy = is_buy && is_sniper;

        self.last_ts_ms = Some(timestamp_ms);
        true
    }

    pub fn analyze(&mut self) -> Option<EctoSignal> {
        if self.events.is_empty() {
            return None;
        }

        let window_ms = self.window_ms();
        let window_denominator = self.config.min_window_ms_emit.max(1);
        let time_fraction = (window_ms as f64 / window_denominator as f64).clamp(0.0, 1.0);
        let min_events = self.config.min_events_for_confidence.max(1) as f64;
        let event_factor = (self.events.len() as f64 / min_events).clamp(0.0, 1.0);
        let confidence = (time_fraction * event_factor).clamp(0.0, 1.0);

        if window_ms < self.config.min_window_ms_emit
            || confidence < self.config.min_confidence_emit
        {
            return None;
        }

        let mut flags = EctoFlags::empty();
        if self.dev_sell_count > 0 {
            flags |= EctoFlags::DEV_SOLD;
        }
        if self.sniper_buy_count >= self.config.sniper_wall_count_threshold
            || self.sniper_buy_streak >= self.config.sniper_wall_streak_threshold
        {
            flags |= EctoFlags::SNIPER_WALL;
        }
        if self.retail_buy_count >= self.config.retail_swarm_count_threshold {
            flags |= EctoFlags::RETAIL_SWARM;
        }
        if self.buy_streak >= self.config.buy_streak_threshold {
            flags |= EctoFlags::BUY_STREAK;
        }

        let mut bias = 0.0;
        if flags.contains(EctoFlags::DEV_SOLD) {
            bias -= 0.7;
        }
        if flags.contains(EctoFlags::SNIPER_WALL) {
            bias -= 0.4;
        }
        if flags.contains(EctoFlags::RETAIL_SWARM) {
            bias += 0.4;
        }
        if flags.contains(EctoFlags::BUY_STREAK) {
            bias += 0.2;
        }

        let total_buy = (self.sniper_buy_count + self.retail_buy_count) as f64;
        if total_buy > 0.0 {
            let retail_ratio = self.retail_buy_count as f64 / total_buy;
            bias += (retail_ratio - 0.5) * 0.4;
        }

        bias = bias.clamp(-1.0, 1.0);

        let score = ((bias + 1.0) * 0.5 * confidence).clamp(0.0, 1.0);

        let verdict = if flags.contains(EctoFlags::DEV_SOLD)
            && confidence >= self.config.min_confidence_emit
        {
            EctoVerdict::Rug
        } else if bias >= 0.35 && confidence >= self.config.min_confidence_emit {
            EctoVerdict::Bullish
        } else {
            EctoVerdict::Neutral
        };

        let signal = EctoSignal {
            bias,
            score,
            confidence,
            flags,
            window_ms,
            verdict,
        };

        self.last_signal = Some(signal);
        self.last_signal
    }

    pub fn window_ms(&self) -> u64 {
        match (self.first_ts_ms, self.last_ts_ms) {
            (Some(first), Some(last)) => last.saturating_sub(first),
            _ => 0,
        }
    }

    pub fn last_signal(&self) -> Option<EctoSignal> {
        self.last_signal
    }

    #[cfg(test)]
    fn event_capacity(&self) -> usize {
        self.events.capacity()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn pk(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    #[test]
    fn detects_dev_sell_in_first_seconds() {
        let mut config = EctoConfig::default();
        config.min_events_for_confidence = 2;
        config.min_window_ms_emit = 1_500;
        let mut state = EctoState::new(config, Some(pk(1)));

        state.update(pk(2), true, 0.5, 1_000, false);
        state.update(pk(1), false, 0.4, 2_600, true);

        let signal = state.analyze().expect("signal should emit");
        assert!(signal.flags.contains(EctoFlags::DEV_SOLD));
        assert!(signal.window_ms >= 1_000);
    }

    #[test]
    fn detects_sniper_wall_from_large_buy_sequence() {
        let mut config = EctoConfig::default();
        config.min_events_for_confidence = 3;
        config.min_window_ms_emit = 1_500;
        let mut state = EctoState::new(config, None);

        state.update(pk(2), true, 1.2, 500, false);
        state.update(pk(3), true, 1.5, 2_000, false);
        state.update(pk(4), true, 1.1, 2_500, false);

        let signal = state.analyze().expect("signal should emit");
        assert!(signal.flags.contains(EctoFlags::SNIPER_WALL));
    }

    #[test]
    fn no_emission_on_low_confidence() {
        let mut config = EctoConfig::default();
        config.min_window_ms_emit = 1_500;
        config.min_confidence_emit = 0.3;
        config.min_events_for_confidence = 6;
        let mut state = EctoState::new(config, None);

        state.update(pk(2), true, 0.1, 200, false);
        state.update(pk(3), true, 0.1, 300, false);

        assert!(state.analyze().is_none());
    }

    #[test]
    fn buffer_capacity_does_not_grow() {
        let config = EctoConfig::default();
        let mut state = EctoState::new(config, None);
        let capacity = state.event_capacity();

        for i in 0..(capacity * 3) {
            state.update(pk((i % 255) as u8), true, 0.3, (i as u64) * 10, false);
        }

        assert_eq!(state.event_capacity(), capacity);
        assert_eq!(state.events.len(), capacity);
    }

    #[test]
    fn bias_stable_when_window_slides_uniformly() {
        let mut config = EctoConfig::default();
        config.buffer_capacity = 4;
        config.min_events_for_confidence = 2;
        config.min_window_ms_emit = 500;
        let mut state = EctoState::new(config, None);

        let pattern = [(true, 0.15), (true, 0.12), (true, 1.2), (true, 0.18)];

        for (i, (is_buy, amount)) in pattern.iter().enumerate() {
            state.update(
                pk((10 + i) as u8),
                *is_buy,
                *amount,
                1_000 + i as u64 * 300,
                false,
            );
        }

        let first = state.analyze().expect("signal should emit");

        state.update(pk(99), true, 0.15, 2_500, false);
        let second = state.analyze().expect("signal should emit");

        assert!((first.bias - second.bias).abs() < 1e-9);
    }

    #[test]
    fn deterministic_for_same_stream() {
        let mut config = EctoConfig::default();
        config.min_events_for_confidence = 2;
        config.min_window_ms_emit = 500;
        let mut a = EctoState::new(config, None);
        let mut b = EctoState::new(config, None);

        let stream = [
            (pk(1), true, 0.1, 500u64),
            (pk(2), true, 1.1, 1_000u64),
            (pk(3), false, 0.3, 1_600u64),
            (pk(4), true, 0.15, 2_100u64),
        ];

        for (signer, is_buy, amount, ts) in stream {
            a.update(signer, is_buy, amount, ts, false);
            b.update(signer, is_buy, amount, ts, false);
        }

        let sa = a.analyze().expect("signal should emit");
        let sb = b.analyze().expect("signal should emit");
        assert_eq!(sa, sb);
    }
}
