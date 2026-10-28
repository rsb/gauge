//! Every closed vocabulary reads its own members back and refuses anything
//! else as an error value, never a default.

#![cfg(test)]

use std::fmt::Debug;
use std::str::FromStr;

use rsb_gauge_reconcile::store::{Agent, EventKind, FlagKind, LoopState, RequestState};
use rsb_gauge_reconcile::{Action, ParkReason, ReconcileError, Refusal, Situation};
use rsb_gauge_schema::Role;

fn closed<T>(all: &[T], as_str: fn(T) -> &'static str)
where
    T: Copy + Debug + PartialEq + FromStr<Err = ReconcileError>,
{
    for &member in all {
        assert_eq!(as_str(member).parse::<T>().ok(), Some(member));
        let json = serde_json::to_value(as_str(member)).unwrap();
        assert!(serde_json::from_value::<String>(json).is_ok());
    }
    for outside in ["", "unknown", "PAUSE", " open", "parked ", "Claude"] {
        assert!(
            matches!(outside.parse::<T>(), Err(ReconcileError::Unknown { .. })),
            "{outside:?} parsed"
        );
    }
}

#[test]
fn agent() {
    closed(Agent::ALL, Agent::as_str);
    assert!(serde_json::from_str::<Agent>("\"gemini\"").is_err());
}

#[test]
fn flag_kind() {
    closed(FlagKind::ALL, FlagKind::as_str);
}

#[test]
fn loop_state() {
    closed(LoopState::ALL, LoopState::as_str);
}

#[test]
fn request_state() {
    closed(RequestState::ALL, RequestState::as_str);
}

#[test]
fn event_kind() {
    closed(EventKind::ALL, EventKind::as_str);
    assert!(serde_json::from_str::<EventKind>("\"loop_closed\"").is_err());
}

#[test]
fn situation() {
    closed(Situation::ALL, Situation::as_str);
}

#[test]
fn park_reason() {
    closed(ParkReason::ALL, ParkReason::as_str);
    assert_eq!(ParkReason::ALL.len(), 8);
}

#[test]
fn action_and_refusal() {
    let park: Action =
        serde_json::from_str(r#"{"action":"park","reason":"unrecognized_state"}"#).unwrap();
    assert_eq!(
        park,
        Action::Park {
            reason: ParkReason::UnrecognizedState
        }
    );
    let refuse: Action =
        serde_json::from_str(r#"{"action":"refuse","refusal":{"reason":"role","role":"factory"}}"#)
            .unwrap();
    assert!(matches!(refuse, Action::Refuse { .. }));
    for outside in [
        r#"{"action":"retry"}"#,
        r#"{"action":"park","reason":"tired"}"#,
        r#"{"action":"refuse","refusal":{"reason":"busy"}}"#,
        r#"{"action":"refuse","refusal":{"reason":"role","role":"vendor"}}"#,
    ] {
        assert!(
            serde_json::from_str::<Action>(outside).is_err(),
            "{outside} parsed"
        );
    }
    assert!(serde_json::from_str::<Refusal>(r#"{"reason":"later"}"#).is_err());
}

#[test]
fn action() {
    let members = [
        (Action::Wait {}, r#"{"action":"wait"}"#),
        (Action::OpenLoop {}, r#"{"action":"open_loop"}"#),
        (
            Action::Refuse {
                refusal: Refusal::NotListed {},
            },
            r#"{"action":"refuse","refusal":{"reason":"not_listed"}}"#,
        ),
        (
            Action::Park {
                reason: ParkReason::NoProgress,
            },
            r#"{"action":"park","reason":"no_progress"}"#,
        ),
    ];
    for (member, json) in members {
        assert_eq!(serde_json::to_string(&member).unwrap(), json);
        assert_eq!(serde_json::from_str::<Action>(json).unwrap(), member);
    }
    for outside in [
        r#"{"action":"wait","until":3}"#,
        r#"{"action":"open_loop","agent":"claude"}"#,
        r#"{"action":"park","reason":"no_progress","loop":3}"#,
        r#"{"action":"refuse","refusal":{"reason":"not_listed"},"loop":3}"#,
        r#"{"action":"Wait"}"#,
        r#"{}"#,
    ] {
        assert!(
            serde_json::from_str::<Action>(outside).is_err(),
            "{outside} parsed"
        );
    }
}

#[test]
fn refusal() {
    let members = [
        (Refusal::NotListed {}, r#"{"reason":"not_listed"}"#),
        (
            Refusal::Role { role: Role::Record },
            r#"{"reason":"role","role":"record"}"#,
        ),
        (
            Refusal::LoopAlreadyOpen {},
            r#"{"reason":"loop_already_open"}"#,
        ),
    ];
    for (member, json) in members {
        assert_eq!(serde_json::to_string(&member).unwrap(), json);
        assert_eq!(serde_json::from_str::<Refusal>(json).unwrap(), member);
    }
    for outside in [
        r#"{"reason":"later"}"#,
        r#"{"reason":"loop_open"}"#,
        r#"{"reason":"Loop_Already_Open"}"#,
        r#"{"reason":"loop_already_open","loop":3}"#,
        r#"{"reason":"not_listed","role":"project"}"#,
        r#"{"reason":"role"}"#,
        r#"{"reason":"role","role":"vendor"}"#,
        r#"{}"#,
    ] {
        assert!(
            serde_json::from_str::<Refusal>(outside).is_err(),
            "{outside} parsed"
        );
    }
}
