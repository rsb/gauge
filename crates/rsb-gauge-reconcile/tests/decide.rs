//! The situation table, row by row and in precedence order.

#![cfg(test)]

use rsb_gauge_reconcile::store::{Agent, LoopId, LoopState, RequestId};
use rsb_gauge_reconcile::{
    Action, Decision, IssueObservation, LoopSeen, Observation, ParkReason, ROWS, Refusal,
    RequestSeen, Situation, decide,
};
use rsb_gauge_schema::Role;

fn issue() -> IssueObservation {
    IssueObservation {
        repo: "ref-rsb-serverless-stack".into(),
        issue: 7,
        paused: false,
        draining: false,
        request: None,
        run: None,
    }
}

fn request(role: Option<Role>) -> Option<RequestSeen> {
    Some(RequestSeen {
        ids: vec![RequestId(1)],
        agent: Agent::Claude,
        role,
    })
}

fn run(state: LoopState) -> Option<LoopSeen> {
    Some(LoopSeen {
        id: LoopId(1),
        state,
        park: (state == LoopState::Parked).then_some(ParkReason::UnrecognizedState),
    })
}

fn decision(situation: Situation, action: Action) -> Decision {
    Decision { situation, action }
}

const WAIT_PAUSED: Decision = Decision {
    situation: Situation::Paused,
    action: Action::Wait,
};
const OPEN: Decision = Decision {
    situation: Situation::RunRequested,
    action: Action::OpenLoop,
};
const PARK: Decision = Decision {
    situation: Situation::Unrecognized,
    action: Action::Park {
        reason: ParkReason::UnrecognizedState,
    },
};

const ALREADY_OPEN: Decision = Decision {
    situation: Situation::RunRequested,
    action: Action::Refuse {
        refusal: Refusal::LoopAlreadyOpen {},
    },
};

fn refuse(refusal: Refusal) -> Decision {
    decision(Situation::Ineligible, Action::Refuse { refusal })
}

#[test]
fn the_table() {
    let cases: Vec<(&str, IssueObservation, Decision)> = vec![
        (
            "13 pause, global or this loop",
            IssueObservation {
                paused: true,
                run: run(LoopState::Open),
                ..issue()
            },
            WAIT_PAUSED,
        ),
        (
            "N1 a factory repo is refused",
            IssueObservation {
                request: request(Some(Role::Factory)),
                ..issue()
            },
            refuse(Refusal::Role {
                role: Role::Factory,
            }),
        ),
        (
            "N1 a record repo is refused",
            IssueObservation {
                request: request(Some(Role::Record)),
                ..issue()
            },
            refuse(Refusal::Role { role: Role::Record }),
        ),
        (
            "N1 a repo absent from repos.toml is refused",
            IssueObservation {
                request: request(None),
                ..issue()
            },
            refuse(Refusal::NotListed {}),
        ),
        (
            "11 a reference repo opens a loop",
            IssueObservation {
                request: request(Some(Role::Reference)),
                ..issue()
            },
            OPEN,
        ),
        (
            "11 a project repo opens a loop",
            IssueObservation {
                request: request(Some(Role::Project)),
                ..issue()
            },
            OPEN,
        ),
        (
            "10 an open loop matches no row yet",
            IssueObservation {
                run: run(LoopState::Open),
                ..issue()
            },
            PARK,
        ),
        (
            "11 a request on an issue whose loop is open is refused",
            IssueObservation {
                request: request(Some(Role::Reference)),
                run: run(LoopState::Open),
                ..issue()
            },
            ALREADY_OPEN,
        ),
        (
            "11 a request on an issue whose loop is parked is refused",
            IssueObservation {
                request: request(Some(Role::Project)),
                run: run(LoopState::Parked),
                ..issue()
            },
            ALREADY_OPEN,
        ),
        (
            "10 a parked loop, before rows 4 and 5 exist",
            IssueObservation {
                run: run(LoopState::Parked),
                ..issue()
            },
            PARK,
        ),
        ("10 nothing at all", issue(), PARK),
    ];
    for (name, observation, expected) in cases {
        assert_eq!(decide(&Observation::Issue(observation)), expected, "{name}");
    }
}

#[test]
fn an_unreachable_store_takes_no_action() {
    assert_eq!(
        decide(&Observation::StoreUnreachable),
        decision(Situation::StoreUnreachable, Action::Wait)
    );
}

#[test]
fn pause_comes_before_refusal_and_opening() {
    for role in [Some(Role::Factory), None, Some(Role::Reference)] {
        let observation = IssueObservation {
            paused: true,
            request: request(role),
            ..issue()
        };
        assert_eq!(decide(&Observation::Issue(observation)), WAIT_PAUSED);
    }
}

#[test]
fn refusal_comes_before_the_open_loop_check() {
    let observation = IssueObservation {
        request: request(Some(Role::Factory)),
        run: run(LoopState::Open),
        ..issue()
    };
    assert_eq!(
        decide(&Observation::Issue(observation)),
        refuse(Refusal::Role {
            role: Role::Factory
        })
    );
}

#[test]
fn drain_gates_only_attempt_starts_so_changes_no_decision_yet() {
    let observations = [
        issue(),
        IssueObservation {
            request: request(Some(Role::Reference)),
            ..issue()
        },
        IssueObservation {
            request: request(Some(Role::Factory)),
            ..issue()
        },
        IssueObservation {
            run: run(LoopState::Open),
            ..issue()
        },
    ];
    for observation in observations {
        let draining = IssueObservation {
            draining: true,
            ..observation.clone()
        };
        assert_eq!(
            decide(&Observation::Issue(draining)),
            decide(&Observation::Issue(observation))
        );
    }
}

#[test]
fn rows_are_in_table_order() {
    let orders: Vec<u8> = ROWS.iter().map(|r| r.order).collect();
    let mut sorted = orders.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(orders, sorted);
    let numbers: Vec<&str> = ROWS.iter().map(|r| r.number).collect();
    assert_eq!(numbers, ["13", "N1", "11"]);
}

#[test]
fn decide_is_a_function_of_its_observation() {
    let observation = Observation::Issue(IssueObservation {
        request: request(Some(Role::Reference)),
        ..issue()
    });
    assert_eq!(decide(&observation), decide(&observation.clone()));
}
