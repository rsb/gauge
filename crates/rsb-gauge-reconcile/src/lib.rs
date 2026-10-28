//! The reconciler's core: what the tick observes, the closed situation table,
//! the pure [`decide`], the events it records, and the store port. No I/O:
//! the driver observes and acts, and the store adapter lives in
//! `rsb-gauge-store`.

mod decide;
mod error;
mod memory;
pub mod observation;
pub mod store;
mod vocabulary;

pub use decide::{
    Action, Decision, ParkReason, ROWS, Refusal, Row, Situation, UNRECOGNIZED, decide,
};
pub use error::ReconcileError;
pub use memory::MemoryStore;
pub use observation::{
    IssueObservation, IssueRows, LoopSeen, Observation, RequestSeen, TicketCause, TicketSeen,
    WorldSeen, issues,
};
pub use store::Store;
