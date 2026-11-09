//! Decision adapters. The legacy sources retain their existing plan and hash semantics.

use std::future::Future;
use std::time::Duration;

use crate::autonomous::{self, AutonomousConfig, Candidate, Preparation, PreparedTrade, Snapshot};
use crate::model::ChatResponse;
use crate::relay::Relay;
use crate::{AgentError, Plan};

pub const MANUAL_SOURCE_ID: &str = "manual-v1";
pub const REBALANCE_SOURCE_ID: &str = "rebalance-v1";

pub enum Decision {
    Trade { plan: Plan, response: Option<Box<ChatResponse>> },
    NoTrade { reason: String },
}

/// Decision sources do not sign transactions or hold treasury authority.
/// The supplied snapshot is the model's public input; legacy adapters delegate
/// their existing routines without changing the legacy I/O or gate behavior.
pub trait DecisionSource: Send + Sync {
    fn decide(&self, snapshot: &Snapshot) -> impl Future<Output = Decision> + Send;
}

pub struct ManualDecision {
    plan: Plan,
}

impl ManualDecision {
    pub fn new(plan: Plan) -> Self {
        Self { plan }
    }

    pub fn validated_plan(&self) -> Result<Plan, AgentError> {
        self.plan.validate()?;
        Ok(self.plan.clone())
    }
}

impl DecisionSource for ManualDecision {
    async fn decide(&self, _snapshot: &Snapshot) -> Decision {
        // Manual plans historically receive structural validation here, followed
        // by the authoritative vault gates at execution. Do not add a new local
        // economic rejection that would change that path.
        match self.validated_plan() {
            Ok(plan) => Decision::Trade { plan, response: None },
            Err(_) => Decision::NoTrade { reason: "manual plan failed existing validation".into() },
        }
    }
}

pub struct RebalanceDecision {
    config: AutonomousConfig,
    relay: Relay,
}

impl RebalanceDecision {
    pub fn new(config: AutonomousConfig, relay: Relay) -> Self {
        Self { config, relay }
    }

    pub fn choose(&self, snapshot: &Snapshot) -> Result<Candidate, AgentError> {
        autonomous::choose(&self.config, snapshot)
    }

    pub async fn prepare(&self) -> Result<Preparation, AgentError> {
        autonomous::prepare(&self.config, &self.relay).await
    }

    pub async fn refresh(&self, prepared: PreparedTrade) -> Result<PreparedTrade, AgentError> {
        autonomous::refresh(&self.config, prepared).await
    }

    /// Public preparation only: no signer, outbox or transaction can be reached here.
    pub async fn prepare_for_submission(&self) -> Result<Preparation, AgentError> {
        match self.prepare().await? {
            Preparation::Trade { prepared } => {
                Ok(Preparation::Trade { prepared: Box::new(self.refresh(*prepared).await?) })
            }
            no_trade => Ok(no_trade),
        }
    }
}

/// A retry is safe only while all work is still public preparation. Submission
/// and outcome errors deliberately do not pass through this retry policy.
pub enum PreparationAttempt {
    Ready(Preparation),
    Retry { after: Duration },
}

pub struct PreparationLoop {
    repeat: bool,
    failures: u32,
}

impl PreparationLoop {
    pub fn new(repeat: bool) -> Self {
        Self { repeat, failures: 0 }
    }

    pub async fn next(
        &mut self,
        source: &RebalanceDecision,
    ) -> Result<PreparationAttempt, AgentError> {
        self.after_preparation(source.prepare_for_submission().await)
    }

    fn after_preparation(
        &mut self,
        result: Result<Preparation, AgentError>,
    ) -> Result<PreparationAttempt, AgentError> {
        match result {
            Ok(prepared) => {
                self.failures = 0;
                Ok(PreparationAttempt::Ready(prepared))
            }
            Err(_) if self.repeat => {
                self.failures = self.failures.saturating_add(1);
                let after = Duration::from_secs((5u64 << self.failures.min(7)).min(600));
                Ok(PreparationAttempt::Retry { after })
            }
            Err(error) => Err(error),
        }
    }
}

impl DecisionSource for RebalanceDecision {
    async fn decide(&self, _snapshot: &Snapshot) -> Decision {
        // Existing prepare obtains its own final snapshot and exact quote.
        // Preserve that behavior instead of introducing a second strategy.
        match self.prepare().await {
            Ok(Preparation::Trade { prepared }) => {
                Decision::Trade { plan: prepared.plan, response: None }
            }
            Ok(Preparation::NoTrade { reason }) => Decision::NoTrade { reason },
            Err(_) => Decision::NoTrade { reason: "existing rebalance preparation failed".into() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_preparation_backoff_is_bounded_and_resets_after_a_success() {
        let mut retries = PreparationLoop::new(true);
        for expected in [10, 20, 40, 80, 160, 320, 600, 600] {
            let PreparationAttempt::Retry { after } =
                retries.after_preparation(Err("untrusted provider detail".into())).unwrap()
            else {
                panic!("public preparation failures must remain retryable")
            };
            assert_eq!(after, Duration::from_secs(expected));
        }
        assert!(matches!(
            retries
                .after_preparation(Ok(Preparation::NoTrade { reason: "paused".into() }))
                .unwrap(),
            PreparationAttempt::Ready(_)
        ));
        let PreparationAttempt::Retry { after } =
            retries.after_preparation(Err("another public read failure".into())).unwrap()
        else {
            panic!("expected a retry")
        };
        assert_eq!(after, Duration::from_secs(10));
        assert!(
            PreparationLoop::new(false).after_preparation(Err("one-shot failure".into())).is_err()
        );
    }
}
