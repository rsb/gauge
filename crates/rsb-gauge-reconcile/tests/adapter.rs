//! The adapter contract suite against in-memory runners: the fake agent's
//! adapter and the Claude adapter pass it, and a broken adapter does not.
//! The driver runs the suite again with runners that commit for real.

#![cfg(test)]

use rsb_gauge_reconcile::adapter::{
    AgentAdapter, AttemptResult, AttemptSpec, ClaudeAdapter, FakeAdapter, Invocation, RunOutput,
    Runner, Script, contract,
};
use rsb_gauge_reconcile::store::Agent;

/// Leaves `commits` made-up commits and prints `stdout` for any script.
struct Canned(fn(&Script) -> Vec<u8>);

impl Runner for Canned {
    fn run(&mut self, _invocation: &Invocation, script: &Script) -> RunOutput {
        RunOutput {
            exit: script.exit,
            stdout: (self.0)(script),
            commits: (0..script.commits).map(|n| format!("{n:040x}")).collect(),
        }
    }
}

#[test]
fn the_fake_agent_passes() {
    let mut runner = Canned(|_| b"{\"step\":\"done\"}\n{\"tokens\":10,\"dollars\":0.5}\n".to_vec());
    assert_eq!(contract(&FakeAdapter, &mut runner), Ok(()));
}

#[test]
fn the_claude_adapter_passes_and_reads_cost_and_tokens() {
    let claude = |_: &Script| {
        br#"{"type":"result","subtype":"success","is_error":false,"result":"done",
             "total_cost_usd":1.25,"usage":{"input_tokens":3,"cache_read_input_tokens":4,
             "cache_creation_input_tokens":5,"output_tokens":6}}"#
            .to_vec()
    };
    assert_eq!(contract(&ClaudeAdapter, &mut Canned(claude)), Ok(()));
    let spec = AttemptSpec {
        unit: "gauge-run-1-1".into(),
        attempt: 1,
        model: "claude-opus-5-5".into(),
        runtime_bound: 60,
    };
    let invocation = ClaudeAdapter.invocation(&spec);
    assert_eq!(invocation.prompt, "/implement /run/brief.json");
    assert_eq!(
        invocation.args,
        ["--output-format", "json", "--model", "claude-opus-5-5"]
    );
    assert!(!invocation.args.iter().any(|a| a == "--bare"), "Q17");
    let usage = ClaudeAdapter
        .result(
            &spec,
            &RunOutput {
                exit: Some(0),
                stdout: claude(&Script {
                    commits: 0,
                    outbox: false,
                    exit: Some(0),
                }),
                commits: vec![],
            },
        )
        .usage
        .unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens, usage.dollars),
        (12, 6, 1.25)
    );
}

/// Reports every attempt as exit 0.
struct Optimist;

impl AgentAdapter for Optimist {
    fn agent(&self) -> Agent {
        Agent::Claude
    }
    fn invocation(&self, _spec: &AttemptSpec) -> Invocation {
        Invocation {
            prompt: "/implement /run/brief.json".into(),
            args: vec![
                "--token".into(),
                "ghs_abcdefghijklmnopqrstuvwxyz0123".into(),
            ],
        }
    }
    fn result(&self, _spec: &AttemptSpec, output: &RunOutput) -> AttemptResult {
        AttemptResult {
            commits: output.commits.clone(),
            exit: Some(0),
            events: None,
            usage: None,
        }
    }
}

#[test]
fn an_adapter_that_misreports_or_carries_a_credential_fails() {
    let failures = contract(&Optimist, &mut Canned(|_| Vec::new())).unwrap_err();
    assert!(
        failures.iter().any(|f| f.contains("credential")),
        "{failures:?}"
    );
    assert!(
        failures.iter().any(|f| f.starts_with("exit non-zero")),
        "{failures:?}"
    );
    assert!(
        failures.iter().any(|f| f.starts_with("killed")),
        "{failures:?}"
    );
}
