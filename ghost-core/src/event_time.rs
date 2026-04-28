use serde::{Deserialize, Serialize};

/// Explicit provenance for the three time axes carried by the pipeline.
///
/// Existing legacy `ts_ms` / `timestamp_ms` fields stay untouched. This struct
/// is additive SSOT metadata used by new code paths to distinguish:
/// - on-chain event time,
/// - ingress wall-clock time,
/// - ingress monotonic process time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventTimeMetadata {
    /// Canonical on-chain event timestamp in epoch milliseconds.
    ///
    /// Must only be present when sourced from chain-native metadata such as
    /// `block_time`. Local fallbacks must never populate this field.
    #[serde(default)]
    pub chain_event_ts_ms: Option<u64>,

    /// Local epoch-millisecond timestamp captured at ingest.
    #[serde(default)]
    pub ingress_wall_ts_ms: Option<u64>,

    /// Local monotonic timestamp captured at ingest.
    #[serde(default)]
    pub ingress_monotonic_ts_ms: Option<u64>,
}

impl EventTimeMetadata {
    pub const fn new(
        chain_event_ts_ms: Option<u64>,
        ingress_wall_ts_ms: Option<u64>,
        ingress_monotonic_ts_ms: Option<u64>,
    ) -> Self {
        Self {
            chain_event_ts_ms,
            ingress_wall_ts_ms,
            ingress_monotonic_ts_ms,
        }
    }

    /// Strict event-axis timestamp: chain first, then ingress wall.
    ///
    /// This never falls back to monotonic time or legacy compatibility fields.
    pub const fn effective_event_ts_ms(self) -> Option<u64> {
        match self.chain_event_ts_ms {
            Some(ts) => Some(ts),
            None => self.ingress_wall_ts_ms,
        }
    }

    /// Event-axis timestamp with explicit compatibility fallback.
    ///
    /// Callers must pass the legacy timestamp intentionally; this helper keeps
    /// old fields usable for replay/older payloads without letting new code
    /// silently forget the provenance distinction.
    pub const fn compat_event_ts_ms(self, legacy_ts_ms: Option<u64>) -> Option<u64> {
        match self.effective_event_ts_ms() {
            Some(ts) => Some(ts),
            None => legacy_ts_ms,
        }
    }

    /// Preserve already-materialized fields while filling any missing axes from
    /// a fallback provenance record.
    pub const fn with_missing_from(self, fallback: Self) -> Self {
        Self {
            chain_event_ts_ms: match self.chain_event_ts_ms {
                Some(ts) => Some(ts),
                None => fallback.chain_event_ts_ms,
            },
            ingress_wall_ts_ms: match self.ingress_wall_ts_ms {
                Some(ts) => Some(ts),
                None => fallback.ingress_wall_ts_ms,
            },
            ingress_monotonic_ts_ms: match self.ingress_monotonic_ts_ms {
                Some(ts) => Some(ts),
                None => fallback.ingress_monotonic_ts_ms,
            },
        }
    }

    pub const fn has_explicit_epoch(self) -> bool {
        self.chain_event_ts_ms.is_some() || self.ingress_wall_ts_ms.is_some()
    }

    pub const fn has_chain_time(self) -> bool {
        self.chain_event_ts_ms.is_some()
    }

    pub const fn is_empty(self) -> bool {
        self.chain_event_ts_ms.is_none()
            && self.ingress_wall_ts_ms.is_none()
            && self.ingress_monotonic_ts_ms.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::EventTimeMetadata;

    #[test]
    fn effective_event_ts_prefers_chain_then_ingress_wall() {
        let with_chain = EventTimeMetadata::new(Some(10), Some(20), Some(30));
        assert_eq!(with_chain.effective_event_ts_ms(), Some(10));

        let ingress_only = EventTimeMetadata::new(None, Some(20), Some(30));
        assert_eq!(ingress_only.effective_event_ts_ms(), Some(20));

        let monotonic_only = EventTimeMetadata::new(None, None, Some(30));
        assert_eq!(monotonic_only.effective_event_ts_ms(), None);
    }

    #[test]
    fn compat_event_ts_uses_legacy_only_when_explicit_epoch_is_missing() {
        let explicit = EventTimeMetadata::new(None, Some(20), Some(30));
        assert_eq!(explicit.compat_event_ts_ms(Some(99)), Some(20));

        let legacy_only = EventTimeMetadata::new(None, None, Some(30));
        assert_eq!(legacy_only.compat_event_ts_ms(Some(99)), Some(99));
    }

    #[test]
    fn with_missing_from_preserves_existing_axes_and_fills_gaps() {
        let primary = EventTimeMetadata::new(None, Some(20), None);
        let fallback = EventTimeMetadata::new(Some(10), Some(99), Some(30));

        assert_eq!(
            primary.with_missing_from(fallback),
            EventTimeMetadata::new(Some(10), Some(20), Some(30))
        );
    }
}
