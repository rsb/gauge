//! The situation table, row by row and in precedence order.

#![cfg(test)]

use rsb_gauge_reconcile::store::{Agent, AttemptCause, AttemptEnd, LoopId, LoopState, RequestId};
use rsb_gauge_reconcile::{
    Action, AttemptSeen, Decision, IssueObservation, LoopSeen, Observation, ParkReason, ROWS,
    RecordedSeen, Refusal, RequestSeen, Situation, TicketCause, TicketSeen, UnitSeen, WorldSeen,
    decide,
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
        attempt: None,
    })
}

/// An open loop whose attempt 1 is in `unit`, recorded unless `recorded` is
/// `None`.
fn attempt(unit: UnitSeen, recorded: Option<RecordedSeen>, sealed: bool) -> IssueObservation {
    attempt_in(LoopState::Open, unit, recorded, sealed)
}

fn attempt_in(
    state: LoopState,
    unit: UnitSeen,
    recorded: Option<RecordedSeen>,
    sealed: bool,
) -> IssueObservation {
    let mut run = run(state);
    if let Some(run) = run.as_mut() {
        run.attempt = Some(AttemptSeen {
            number: 1,
            brief: true,
            recorded,
            unit,
            sealed,
        });
    }
    IssueObservation { run, ..issue() }
}

fn recorded(past_bound: bool) -> Option<RecordedSeen> {
    Some(RecordedSeen {
        past_bound,
        end: None,
    })
}

fn ended(cause: Option<AttemptCause>) -> Option<RecordedSeen> {
    Some(RecordedSeen {
        past_bound: true,
        end: Some(AttemptEnd {
            at: 0,
            exit: None,
            cause,
        }),
    })
}

const START_1: Decision = Decision {
    situation: Situation::AttemptDue,
    action: Action::StartAttempt { attempt: 1 },
};

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
            "N6 an open loop with no attempt starts attempt 1",
            IssueObservation {
                run: run(LoopState::Open),
                ..issue()
            },
            START_1,
        ),
        (
            "N6 a brief issued, no unit, no record: start it (kill point 1)",
            attempt(UnitSeen::Absent, None, false),
            START_1,
        ),
        (
            "N6 a unit started, not recorded: start it, which records it",
            attempt(UnitSeen::Running, None, false),
            START_1,
        ),
        (
            "1 running within its bound waits",
            attempt(UnitSeen::Running, recorded(false), false),
            decision(Situation::AttemptRunning, Action::Wait {}),
        ),
        (
            "N2 running past its bound is stopped, timed_out",
            attempt(UnitSeen::Running, recorded(true), false),
            decision(
                Situation::AttemptOverBound,
                Action::StopAttempt {
                    attempt: 1,
                    cause: AttemptCause::TimedOut,
                },
            ),
        ),
        (
            "N2 still running after its end was recorded is stopped again",
            attempt(
                UnitSeen::Running,
                ended(Some(AttemptCause::TimedOut)),
                false,
            ),
            decision(
                Situation::AttemptOverBound,
                Action::StopAttempt {
                    attempt: 1,
                    cause: AttemptCause::TimedOut,
                },
            ),
        ),
        (
            "N13 exited non-zero with no sealed outbox: exited_nonzero",
            attempt(UnitSeen::Exited { code: Some(2) }, recorded(false), false),
            decision(
                Situation::AttemptExited,
                Action::EndAttempt {
                    attempt: 1,
                    exit: Some(2),
                    cause: Some(AttemptCause::ExitedNonzero),
                },
            ),
        ),
        (
            "N13 ended by a signal with no sealed outbox: exited_nonzero",
            attempt(UnitSeen::Exited { code: None }, recorded(true), false),
            decision(
                Situation::AttemptExited,
                Action::EndAttempt {
                    attempt: 1,
                    exit: None,
                    cause: Some(AttemptCause::ExitedNonzero),
                },
            ),
        ),
        (
            "N13 exited non-zero with a sealed outbox: no cause (07 reads it)",
            attempt(UnitSeen::Exited { code: Some(1) }, recorded(false), true),
            decision(
                Situation::AttemptExited,
                Action::EndAttempt {
                    attempt: 1,
                    exit: Some(1),
                    cause: None,
                },
            ),
        ),
        (
            "N13 exited zero: no cause",
            attempt(UnitSeen::Exited { code: Some(0) }, recorded(false), false),
            decision(
                Situation::AttemptExited,
                Action::EndAttempt {
                    attempt: 1,
                    exit: Some(0),
                    cause: None,
                },
            ),
        ),
        (
            "28 an ended attempt matches no row until ticket 07",
            attempt(
                UnitSeen::Exited { code: Some(2) },
                ended(Some(AttemptCause::ExitedNonzero)),
                false,
            ),
            PARK,
        ),
        (
            "28 a recorded attempt whose unit is gone matches no row",
            attempt(UnitSeen::Absent, recorded(false), false),
            PARK,
        ),
        (
            "28 a parked loop's running attempt is not touched by rows 1 to N6",
            attempt_in(LoopState::Parked, UnitSeen::Running, recorded(true), false),
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
fn drain_holds_a_start_but_not_the_record_of_a_started_unit() {
    let draining = |o: IssueObservation| {
        decide(&Observation::Issue(IssueObservation {
            draining: true,
            ..o
        }))
    };
    let wait = decision(Situation::Draining, Action::Wait {});
    assert_eq!(
        draining(IssueObservation {
            run: run(LoopState::Open),
            ..issue()
        }),
        wait
    );
    assert_eq!(draining(attempt(UnitSeen::Absent, None, false)), wait);
    assert_eq!(draining(attempt(UnitSeen::Running, None, false)), START_1);
    // What runs goes on: drain changes nothing for a recorded attempt.
    for o in [
        attempt(UnitSeen::Running, recorded(false), false),
        attempt(UnitSeen::Running, recorded(true), false),
        attempt(UnitSeen::Exited { code: Some(2) }, recorded(false), false),
    ] {
        assert_eq!(draining(o.clone()), decide(&Observation::Issue(o)));
    }
}

#[test]
fn drain_changes_no_decision_but_a_start() {
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
    assert_eq!(
        numbers,
        ["13", "N1", "N11", "N12", "11", "1", "N2", "N13", "N6", "14"]
    );
}

#[test]
fn decide_is_a_function_of_its_observation() {
    let observation = Observation::Issue(IssueObservation {
        request: request(Some(Role::Reference)),
        ..issue()
    });
    assert_eq!(decide(&observation), decide(&observation.clone()));
}
