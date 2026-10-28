use crate::store::LoopId;

/// Why a reconcile value could not be read or a store write was refused.
#[derive(Debug, thiserror::Error)]
pub enum ReconcileError {
    /// A value from outside a closed vocabulary. Never read as a default.
    #[error("{value:?} is not a known {vocabulary}")]
    Unknown {
        vocabulary: &'static str,
        value: String,
    },
    #[error("there is no loop {0}")]
    NoLoop(LoopId),
}
