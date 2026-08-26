//! Explicit failure authority for bounded, observe-only ACE captures.
//!
//! The classifier is deliberately small: it is not a retry framework and it
//! does not turn data loss into success.  Its job is to make every component
//! state whether a failure is local, an optional dependency problem, a
//! recoverable transport gap, a capture-segment invalidation, or an actual
//! process-wide integrity failure.

/// The only classes permitted at runtime boundaries which can otherwise
/// acquire process shutdown or global candidate-admission authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureFailureClassV1 {
    /// The affected candidate is blocked/reclaimed; unrelated candidates keep
    /// their normal admission authority.
    CandidateLocal,
    /// A non-canonical evidence or diagnostics lane is unavailable.
    OptionalLaneDegraded,
    /// A transient external request failed (timeout, rate limit, DNS, reset,
    /// or temporary HTTP status).  The caller owns bounded retry/backoff.
    TransientExternalDependency,
    /// A bounded delivery path has a recoverable incident.  Evidence is
    /// retained and the caller may later prove or invalidate the affected
    /// segment; it cannot directly terminate the launcher.
    RecoverableTransportGap,
    /// Canonical evidence for one interval cannot be proved complete.  The
    /// launcher continues to preserve later evidence, while finalization
    /// rejects the capture through a durable counter.
    CaptureSegmentInvalid,
    /// Only internal corruption, a non-recoverable registry failure, durable
    /// canonical-tape failure after bounded recovery, core task death, or an
    /// explicit operator shutdown may retain global authority.
    GlobalRuntimeFatal,
}

impl CaptureFailureClassV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CandidateLocal => "candidate_local",
            Self::OptionalLaneDegraded => "optional_lane_degraded",
            Self::TransientExternalDependency => "transient_external_dependency",
            Self::RecoverableTransportGap => "recoverable_transport_gap",
            Self::CaptureSegmentInvalid => "capture_segment_invalid",
            Self::GlobalRuntimeFatal => "global_runtime_fatal",
        }
    }

    /// A global admission transition is reserved for actual integrity
    /// corruption.  Segment-invalid captures are intentionally allowed to
    /// continue collecting later tape for forensic evidence.
    pub const fn closes_candidate_admission(self) -> bool {
        matches!(self, Self::GlobalRuntimeFatal)
    }

    pub const fn invalidates_capture_segment(self) -> bool {
        matches!(self, Self::CaptureSegmentInvalid)
    }
}

/// Record a typed failure without granting the caller implicit shutdown
/// authority.  `code` is a static callsite label, never raw RPC text.
pub fn record_capture_failure(class: CaptureFailureClassV1, code: &'static str) {
    crate::oracle_metrics::record_ace_capture_failure(class.as_str(), code);
    if class.invalidates_capture_segment() {
        crate::oracle_metrics::record_ace_capture_segment_invalid();
    }
}

#[cfg(test)]
mod tests {
    use super::CaptureFailureClassV1;

    #[test]
    fn only_true_global_fatal_has_admission_close_authority() {
        for class in [
            CaptureFailureClassV1::CandidateLocal,
            CaptureFailureClassV1::OptionalLaneDegraded,
            CaptureFailureClassV1::TransientExternalDependency,
            CaptureFailureClassV1::RecoverableTransportGap,
            CaptureFailureClassV1::CaptureSegmentInvalid,
        ] {
            assert!(!class.closes_candidate_admission());
        }
        assert!(CaptureFailureClassV1::GlobalRuntimeFatal.closes_candidate_admission());
    }
}
