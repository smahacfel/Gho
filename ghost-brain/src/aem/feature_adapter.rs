use trigger::{
    AemCommandDirective as TriggerDirective, AemCommandPriority as TriggerPriority, Revolver,
};

use crate::aem::types::{
    ActionChosen, CommandApplyResult, CommandDirective, CommandPriority, ControlCommand,
    ExecutionStressSnapshot, TriggerControlAdapter,
};

#[derive(Debug, Default, Clone)]
pub struct FeatureBuilder;

#[derive(Debug)]
pub struct RevolverAemAdapter<'a> {
    revolver: &'a mut Revolver,
}

impl<'a> RevolverAemAdapter<'a> {
    pub fn new(revolver: &'a mut Revolver) -> Self {
        Self { revolver }
    }
}

impl<'a> TriggerControlAdapter for RevolverAemAdapter<'a> {
    fn apply_control_command(
        &mut self,
        cmd: &ControlCommand,
        now_unix_ms: u64,
    ) -> CommandApplyResult {
        let priority = match cmd.priority {
            CommandPriority::Default => TriggerPriority::Default,
            CommandPriority::AemPolicy => TriggerPriority::AemPolicy,
            CommandPriority::HardSafety => TriggerPriority::HardSafety,
        };
        let directive = match cmd.directive {
            CommandDirective::Noop => TriggerDirective::Noop,
            CommandDirective::SetTightStop => TriggerDirective::SetTightStop,
            CommandDirective::SetLooseStop => TriggerDirective::SetLooseStop,
            CommandDirective::ForceExitAll => TriggerDirective::ForceExitAll,
            CommandDirective::ForceExitFractionBps { fraction_bps } => {
                TriggerDirective::ForceExitFractionBps { fraction_bps }
            }
            CommandDirective::FreezePanic => TriggerDirective::FreezePanic,
        };
        let out = self.revolver.apply_aem_control_command(
            &cmd.position_id,
            cmd.position_epoch,
            cmd.issued_at_unix_ms,
            cmd.valid_from_unix_ms,
            cmd.expires_at_unix_ms,
            priority,
            directive,
            &cmd.reason_code,
            now_unix_ms,
        );
        CommandApplyResult {
            accepted: out.accepted,
            reject_reason: out.reject_reason,
        }
    }

    fn get_execution_stress(&self, position_id: &str) -> Option<ExecutionStressSnapshot> {
        self.revolver
            .get_execution_stress_by_position(position_id)
            .map(|s| ExecutionStressSnapshot {
                requeue_count: s.requeue_count,
                send_fail_count: s.send_fail_count,
                relax_count: s.relax_count,
                oracle_stale_age_ms: s.oracle_stale_age_ms,
                last_sell_attempt_age_ms: s.last_sell_attempt_age_ms,
            })
    }

    fn register_position_epoch(&mut self, position_id: &str, position_epoch: u64) {
        self.revolver
            .register_position_epoch(position_id, position_epoch);
    }

    fn unregister_position_epoch(&mut self, position_id: &str) {
        self.revolver.unregister_position_epoch(position_id);
    }
}

pub fn choose_default_directive(
    action: ActionChosen,
    partial_fraction_bps: u16,
) -> CommandDirective {
    match action {
        ActionChosen::SellNow | ActionChosen::Panic => CommandDirective::ForceExitAll,
        ActionChosen::WaitReclaim => CommandDirective::FreezePanic,
        ActionChosen::Partial => CommandDirective::ForceExitFractionBps {
            fraction_bps: partial_fraction_bps,
        },
    }
}
