use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    Grpc,
    GeyserWs,
    HeliusWs,
    PumpPortal,
}

impl SourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grpc => "grpc",
            Self::GeyserWs => "geyser_ws",
            Self::HeliusWs => "helius_ws",
            Self::PumpPortal => "pumpportal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventTruthKind {
    #[default]
    RawChain,
    AdaptedChain,
    Synthetic,
}

impl EventTruthKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RawChain => "raw_chain",
            Self::AdaptedChain => "adapted_chain",
            Self::Synthetic => "synthetic",
        }
    }

    pub const fn is_synthetic(self) -> bool {
        matches!(self, Self::Synthetic)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SlotQuality {
    Present,
    #[default]
    Absent,
    Estimated,
}

impl SlotQuality {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimestampQuality {
    Chain,
    Adapter,
    #[default]
    WallClock,
}

impl TimestampQuality {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chain => "chain",
            Self::Adapter => "adapter",
            Self::WallClock => "wall_clock",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventCompleteness {
    Full,
    #[default]
    Partial,
}

impl EventCompleteness {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventSemanticEnvelope {
    pub source_kind: SourceKind,
    pub event_truth_kind: EventTruthKind,
    pub slot_quality: SlotQuality,
    pub timestamp_quality: TimestampQuality,
    pub completeness: EventCompleteness,
}

impl EventSemanticEnvelope {
    pub const fn new(
        source_kind: SourceKind,
        event_truth_kind: EventTruthKind,
        slot_quality: SlotQuality,
        timestamp_quality: TimestampQuality,
        completeness: EventCompleteness,
    ) -> Self {
        Self {
            source_kind,
            event_truth_kind,
            slot_quality,
            timestamp_quality,
            completeness,
        }
    }

    pub const fn with_timestamp_quality(self, timestamp_quality: TimestampQuality) -> Self {
        Self {
            timestamp_quality,
            completeness: derive_completeness(
                self.event_truth_kind,
                self.slot_quality,
                timestamp_quality,
            ),
            ..self
        }
    }

    pub const fn is_synthetic(self) -> bool {
        self.event_truth_kind.is_synthetic()
    }
}

pub const fn source_kind_from_label(source: &str) -> SourceKind {
    if starts_with(source, "helius") {
        SourceKind::HeliusWs
    } else if str_eq(source, "websocket") {
        SourceKind::GeyserWs
    } else if str_eq(source, "pumpportal") {
        SourceKind::PumpPortal
    } else {
        SourceKind::Grpc
    }
}

pub const fn normalize_transaction_semantics(
    source: &str,
    synthetic: bool,
    slot_present: bool,
    timestamp_quality: TimestampQuality,
) -> EventSemanticEnvelope {
    let source_kind = source_kind_from_label(source);
    let event_truth_kind = if synthetic {
        EventTruthKind::Synthetic
    } else {
        match source_kind {
            SourceKind::Grpc => EventTruthKind::RawChain,
            SourceKind::GeyserWs | SourceKind::HeliusWs => EventTruthKind::AdaptedChain,
            SourceKind::PumpPortal => EventTruthKind::Synthetic,
        }
    };
    let slot_quality = if slot_present {
        SlotQuality::Present
    } else {
        SlotQuality::Absent
    };
    EventSemanticEnvelope::new(
        source_kind,
        event_truth_kind,
        slot_quality,
        timestamp_quality,
        derive_completeness(event_truth_kind, slot_quality, timestamp_quality),
    )
}

pub const fn normalize_account_update_semantics(
    source: &str,
    slot_present: bool,
) -> EventSemanticEnvelope {
    let source_kind = source_kind_from_label(source);
    let slot_quality = if slot_present {
        SlotQuality::Present
    } else {
        SlotQuality::Absent
    };
    EventSemanticEnvelope::new(
        source_kind,
        EventTruthKind::RawChain,
        slot_quality,
        TimestampQuality::WallClock,
        EventCompleteness::Partial,
    )
}

pub fn record_event_semantic_metric(semantic: EventSemanticEnvelope) {
    ::metrics::increment_counter!(
        "event_semantic_total",
        "source_kind" => semantic.source_kind.as_str(),
        "truth_kind" => semantic.event_truth_kind.as_str(),
        "slot_quality" => semantic.slot_quality.as_str(),
        "timestamp_quality" => semantic.timestamp_quality.as_str(),
        "completeness" => semantic.completeness.as_str()
    );
}

const fn derive_completeness(
    event_truth_kind: EventTruthKind,
    slot_quality: SlotQuality,
    timestamp_quality: TimestampQuality,
) -> EventCompleteness {
    if matches!(event_truth_kind, EventTruthKind::RawChain)
        && matches!(slot_quality, SlotQuality::Present)
        && !matches!(timestamp_quality, TimestampQuality::WallClock)
    {
        EventCompleteness::Full
    } else {
        EventCompleteness::Partial
    }
}

const fn starts_with(value: &str, prefix: &str) -> bool {
    let value = value.as_bytes();
    let prefix = prefix.as_bytes();
    if prefix.len() > value.len() {
        return false;
    }
    let mut idx = 0;
    while idx < prefix.len() {
        if value[idx] != prefix[idx] {
            return false;
        }
        idx += 1;
    }
    true
}

const fn str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut idx = 0;
    while idx < left.len() {
        if left[idx] != right[idx] {
            return false;
        }
        idx += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_account_update_semantics, normalize_transaction_semantics, EventCompleteness,
        EventTruthKind, SlotQuality, SourceKind, TimestampQuality,
    };

    #[test]
    fn normalizes_grpc_raw_chain_event() {
        let semantic = normalize_transaction_semantics(
            "grpc_global_stream",
            false,
            true,
            TimestampQuality::Chain,
        );
        assert_eq!(semantic.source_kind, SourceKind::Grpc);
        assert_eq!(semantic.event_truth_kind, EventTruthKind::RawChain);
        assert_eq!(semantic.slot_quality, SlotQuality::Present);
        assert_eq!(semantic.completeness, EventCompleteness::Full);
    }

    #[test]
    fn normalizes_geyser_websocket_as_adapted() {
        let semantic =
            normalize_transaction_semantics("websocket", false, true, TimestampQuality::Adapter);
        assert_eq!(semantic.source_kind, SourceKind::GeyserWs);
        assert_eq!(semantic.event_truth_kind, EventTruthKind::AdaptedChain);
        assert_eq!(semantic.completeness, EventCompleteness::Partial);
    }

    #[test]
    fn normalizes_helius_as_adapted() {
        let semantic =
            normalize_transaction_semantics("helius", false, true, TimestampQuality::Adapter);
        assert_eq!(semantic.source_kind, SourceKind::HeliusWs);
        assert_eq!(semantic.event_truth_kind, EventTruthKind::AdaptedChain);
        assert_eq!(semantic.completeness, EventCompleteness::Partial);
    }

    #[test]
    fn normalizes_pumpportal_as_synthetic() {
        let semantic =
            normalize_transaction_semantics("pumpportal", true, false, TimestampQuality::Adapter);
        assert_eq!(semantic.source_kind, SourceKind::PumpPortal);
        assert_eq!(semantic.event_truth_kind, EventTruthKind::Synthetic);
        assert_eq!(semantic.slot_quality, SlotQuality::Absent);
        assert_eq!(semantic.completeness, EventCompleteness::Partial);
    }

    #[test]
    fn account_update_is_partial_without_chain_timestamp() {
        let semantic = normalize_account_update_semantics("grpc_global_stream", true);
        assert_eq!(semantic.event_truth_kind, EventTruthKind::RawChain);
        assert_eq!(semantic.timestamp_quality, TimestampQuality::WallClock);
        assert_eq!(semantic.completeness, EventCompleteness::Partial);
    }
}
