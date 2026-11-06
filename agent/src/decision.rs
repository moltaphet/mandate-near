//! Decision adapters. The legacy sources retain their existing plan and hash semantics.

use std::future::Future;

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
