use crate::aem::{
    config::AemConfig,
    error::AemError,
    types::{
        ActionChosen, ActionStats, ManagementDecisionEvent, ManagementOutcomeEvent, RegimeBook,
        RegimeKey, ReplayPair, UnixMs,
    },
};

#[derive(Debug, Clone, Default)]
struct RunningStats {
    weight_sum: f64,
    mean: f64,
    m2: f64,
    tail_weight: f64,
    count_u32: u32,
}

impl RunningStats {
    fn push_weighted(&mut self, x: f64, w: f64, tail_loss: bool) {
        if !x.is_finite() || !w.is_finite() || w <= 0.0 {
            return;
        }
        let new_weight_sum = self.weight_sum + w;
        let delta = x - self.mean;
        let r = w / new_weight_sum;
        self.mean += r * delta;
        self.m2 += w * delta * (x - self.mean);
        self.weight_sum = new_weight_sum;
        if tail_loss {
            self.tail_weight += w;
        }
        self.count_u32 = self.count_u32.saturating_add(1);
    }

    fn into_action_stats(self) -> ActionStats {
        let variance = if self.weight_sum > 0.0 {
            self.m2 / self.weight_sum
        } else {
            0.0
        };
        let std = if variance.is_finite() && variance >= 0.0 {
            Some(variance.sqrt())
        } else {
            None
        };
        let tail = if self.weight_sum > 0.0 {
            Some((self.tail_weight / self.weight_sum).clamp(0.0, 1.0))
        } else {
            None
        };
        ActionStats {
            n: self.count_u32,
            mean_delta_pnl: if self.mean.is_finite() {
                self.mean
            } else {
                0.0
            },
            std_delta_pnl: std,
            tail_risk_rate: tail,
        }
    }
}

impl RegimeBook {
    pub fn estimate(&self, key: &RegimeKey, action: ActionChosen) -> Option<&ActionStats> {
        self.stats.get(&(key.clone(), action))
    }

    pub fn update_from_outcome(
        &mut self,
        decision: &ManagementDecisionEvent,
        outcome: &ManagementOutcomeEvent,
        now_unix_ms: UnixMs,
        cfg: &AemConfig,
    ) -> Result<(), AemError> {
        if !outcome.counterfactual_delta_pnl.is_finite() {
            return Err(AemError::InvalidData(
                "counterfactual_delta_pnl is not finite".to_string(),
            ));
        }
        let lambda = cfg.decay_lambda();
        let age_days =
            ((now_unix_ms.saturating_sub(outcome.timestamp_outcome_unix_ms)) as f64) / 86_400_000.0;
        let weight = (-lambda * age_days.max(0.0)).exp();

        let key = (decision.regime_key.clone(), decision.action_chosen);
        let existing = self.stats.get(&key).cloned().unwrap_or_default();

        let mut running = RunningStats {
            weight_sum: existing.n as f64,
            mean: existing.mean_delta_pnl,
            m2: existing
                .std_delta_pnl
                .map(|s| s * s * (existing.n as f64))
                .unwrap_or(0.0),
            tail_weight: existing
                .tail_risk_rate
                .map(|t| t * (existing.n as f64))
                .unwrap_or(0.0),
            count_u32: existing.n,
        };
        running.push_weighted(
            outcome.counterfactual_delta_pnl,
            weight,
            outcome.tail_loss_flag,
        );
        self.stats.insert(key, running.into_action_stats());
        Ok(())
    }

    pub fn rebuild_from_replay(
        replay: Vec<ReplayPair>,
        now_unix_ms: UnixMs,
        cfg: &AemConfig,
    ) -> Result<Self, AemError> {
        let mut rb = RegimeBook::default();
        let mut replay_sorted = replay;
        replay_sorted.sort_by(|a, b| {
            a.outcome
                .timestamp_outcome_unix_ms
                .cmp(&b.outcome.timestamp_outcome_unix_ms)
                .then(a.outcome.outcome_event_id.cmp(&b.outcome.outcome_event_id))
        });
        for pair in replay_sorted {
            rb.update_from_outcome(&pair.decision, &pair.outcome, now_unix_ms, cfg)?;
        }
        Ok(rb)
    }
}
