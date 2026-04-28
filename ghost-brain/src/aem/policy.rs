use crate::aem::{
    config::AemConfig,
    error::AemError,
    types::{
        ActionChosen, CiBounds, PolicyDecision, PolicyEngine, RegimeBook, RegimeKey, RegimeTag,
        StateFeatures, StressBucket,
    },
};

impl PolicyEngine {
    pub fn decide(
        &self,
        features: &StateFeatures,
        _regime_tag: RegimeTag,
        regime_key: &RegimeKey,
        regime_book: &RegimeBook,
        cfg: &AemConfig,
    ) -> PolicyDecision {
        let wait = regime_book.estimate(regime_key, ActionChosen::WaitReclaim);
        let sell = regime_book.estimate(regime_key, ActionChosen::SellNow);
        let partial = regime_book.estimate(regime_key, ActionChosen::Partial);

        let wait_ci = wait.and_then(|s| {
            if s.n < 2 {
                return None;
            }
            s.std_delta_pnl
                .and_then(|std| Self::ci_bounds(s.mean_delta_pnl, std, s.n, cfg.k_ci).ok())
        });
        let sell_ci = sell.and_then(|s| {
            if s.n < 2 {
                return None;
            }
            s.std_delta_pnl
                .and_then(|std| Self::ci_bounds(s.mean_delta_pnl, std, s.n, cfg.k_ci).ok())
        });
        let partial_ci = partial.and_then(|s| {
            if s.n < 2 {
                return None;
            }
            s.std_delta_pnl
                .and_then(|std| Self::ci_bounds(s.mean_delta_pnl, std, s.n, cfg.k_ci).ok())
        });

        let stress_low = features.stress_bucket == StressBucket::Low;
        let oracle_fresh = features.oracle_stale_age_ms < cfg.oracle_stale_hard_ms;
        let enough_n = wait.map(|s| s.n >= cfg.n_min_per_key).unwrap_or(false);
        let tail_ok = wait
            .and_then(|s| s.tail_risk_rate)
            .map(|v| v <= cfg.tail_risk_limit_wait)
            .unwrap_or(false);
        let lcb_edge_ok = wait_ci
            .as_ref()
            .map(|w| w.lcb >= cfg.min_edge)
            .unwrap_or(false);

        let cmp_sell_ok = if cfg.ci_compare_against_sell_now {
            match (wait_ci.as_ref(), sell_ci.as_ref()) {
                (Some(w), Some(s)) => w.lcb > s.ucb,
                _ => false,
            }
        } else {
            true
        };
        let cmp_partial_ok = if cfg.ci_compare_against_partial {
            match (wait_ci.as_ref(), partial_ci.as_ref()) {
                (Some(w), Some(p)) => w.lcb > p.ucb,
                _ => false,
            }
        } else {
            true
        };

        let ci_pass = stress_low
            && oracle_fresh
            && enough_n
            && tail_ok
            && lcb_edge_ok
            && cmp_sell_ok
            && cmp_partial_ok;

        if ci_pass {
            return PolicyDecision {
                action_chosen: ActionChosen::WaitReclaim,
                reason_code: "wait_reclaim_ci_pass".to_string(),
                wait_ci,
                sell_ci,
                partial_ci,
                ci_check_passed: true,
            };
        }

        let action = if features.drawdown_pct >= 60.0 {
            ActionChosen::SellNow
        } else {
            ActionChosen::Partial
        };
        let reason = if !stress_low {
            "fallback_stress_not_low"
        } else if !oracle_fresh {
            "fallback_oracle_stale"
        } else if !enough_n {
            "fallback_n_too_low"
        } else {
            "fallback_ci_fail"
        };

        PolicyDecision {
            action_chosen: action,
            reason_code: reason.to_string(),
            wait_ci,
            sell_ci,
            partial_ci,
            ci_check_passed: false,
        }
    }

    pub fn ci_bounds(mean: f64, std: f64, n: u32, k: f64) -> Result<CiBounds, AemError> {
        if !mean.is_finite() || !std.is_finite() || !k.is_finite() {
            return Err(AemError::InvalidData(
                "mean/std/k must be finite".to_string(),
            ));
        }
        if n < 2 || std < 0.0 || k <= 0.0 {
            return Err(AemError::InvalidData(
                "invalid ci bounds parameters".to_string(),
            ));
        }
        let se = std / (n as f64).sqrt();
        let lcb = mean - (k * se);
        let ucb = mean + (k * se);
        if !lcb.is_finite() || !ucb.is_finite() {
            return Err(AemError::InvalidData(
                "ci bounds are not finite".to_string(),
            ));
        }
        Ok(CiBounds { lcb, ucb, k, n })
    }
}
