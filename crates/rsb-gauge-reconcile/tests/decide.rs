//! The situation table, row by row and in precedence order.

#![cfg(test)]

use rsb_gauge_reconcile::store::{Agent, LoopId, LoopState, RequestId};
use rsb_gauge_reconcile::{
    Action, Decision, IssueObservation, LoopSeen, Observation, ParkReason, ROWS, Refusal,
    RequestSeen, Situation, TicketCause, TicketSeen, WorldSeen, decide,
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

/// A minted repo's default branch, with the issue naming a ticket file.
fn minted() -> WorldSeen {
    WorldSeen {
        branch: "main".into(),
        sha: "0123456789abcdef0123456789abcdef01234567".into(),
        lockfile: Some("ab".repeat(32)),
        ticket: TicketSeen::Resolved {
            id: "S-0001/5".into(),
            path: "specs/S-0001-thin-loop/05-world-observed.md".into(),
        },
    }
}

/// A request as the tick observes it: the world is read for a repo eligible
/// by role, so it is the minted one there, and `None` elsewhere.
fn request(role: Option<Role>) -> Option<RequestSeen> {
    let world = role.filter(|r| r.is_eligible()).map(|_| minted());
    request_in(role, world)
}

fn request_in(role: Option<Role>, world: Option<WorldSeen>) -> Option<RequestSeen> {
    Some(RequestSeen {
        ids: vec![RequestId(1)],
        agent: Agent::Claude,
        role,
        world,
    })
}

fn no_lockfile() -> WorldSeen {
    WorldSeen {
        lockfile: None,
        ..minted()
    }
}

fn unresolved(cause: TicketCause) -> WorldSeen {
    WorldSeen {
        ticket: TicketSeen::Unresolved { cause },
        ..minted()
    }
}

fn no_ticket(cause: TicketCause) -> Decision {
    decision(
        Situation::TicketUnresolved,
        Action::Refuse {
            refusal: Refusal::NoTicket { cause },
        },
    )
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
    action: Action::Wait {},
};
const OPEN: Decision = Decision {
    situation: Situation::RunRequested,
    action: Action::OpenLoop {},
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
            "N1 a repo eligible by role that the driver does not reach is refused",
            IssueObservation {
                request: request_in(Some(Role::Project), None),
                ..issue()
            },
            refuse(Refusal::NotReached {}),
        ),
        (
            "N1 a repo with no lockfile on its default branch is refused",
            IssueObservation {
                request: request_in(Some(Role::Reference), Some(no_lockfile())),
                ..issue()
            },
            refuse(Refusal::NoLockfile {}),
        ),
        (
            "N1 the lockfile comes before the ticket",
            IssueObservation {
                request: request_in(
                    Some(Role::Reference),
                    Some(WorldSeen {
                        lockfile: None,
                        ..unresolved(TicketCause::NoMarker)
                    }),
                ),
                ..issue()
            },
            refuse(Refusal::NoLockfile {}),
        ),
        (
            "N12 an issue naming no ticket is refused, not parked",
            IssueObservation {
                request: request_in(
                    Some(Role::Reference),
                    Some(unresolved(TicketCause::NoMarker)),
                ),
                ..issue()
            },
            no_ticket(TicketCause::NoMarker),
        ),
        (
            "N12 an epic is refused",
            IssueObservation {
                request: request_in(Some(Role::Project), Some(unresolved(TicketCause::Epic))),
                ..issue()
            },
            no_ticket(TicketCause::Epic),
        ),
        (
            "N12 a ticket with no file is refused",
            IssueObservation {
                request: request_in(
                    Some(Role::Project),
                    Some(unresolved(TicketCause::NoTicketFile)),
                ),
                ..issue()
            },
            no_ticket(TicketCause::NoTicketFile),
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
            "N11 a request on an issue whose loop is open is refused",
            IssueObservation {
                request: request(Some(Role::Reference)),
                run: run(LoopState::Open),
                ..issue()
            },
            ALREADY_OPEN,
        ),
        (
            "N11 a request on an issue whose loop is parked is refused",
            IssueObservation {
                request: request(Some(Role::Project)),
                run: run(LoopState::Parked),
                ..issue()
            },
            ALREADY_OPEN,
        ),
        (
            "N1 a lockfile gone refuses a second request before N11",
            IssueObservation {
                request: request_in(Some(Role::Project), Some(no_lockfile())),
                run: run(LoopState::Open),
                ..issue()
            },
            refuse(Refusal::NoLockfile {}),
        ),
        (
            "N11 comes before N12: the open loop is the reason",
            IssueObservation {
                request: request_in(
                    Some(Role::Project),
                    Some(unresolved(TicketCause::IssueClosed)),
                ),
                run: run(LoopState::Open),
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
        decision(Situation::StoreUnreachable, Action::Wait {})
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
    assert_eq!(orders, sorted);
    let numbers: Vec<&str> = ROWS.iter().map(|r| r.number).collect();
    assert_eq!(numbers, ["13", "N1", "N11", "N12", "11"]);
}

#[test]
fn decide_is_a_function_of_its_observation() {
    let observation = Observation::Issue(IssueObservation {
        request: request(Some(Role::Reference)),
        ..issue()
    });
    assert_eq!(decide(&observation), decide(&observation.clone()));
}
