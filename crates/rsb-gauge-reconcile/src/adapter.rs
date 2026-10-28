//! The agent adapter: how an attempt's agent is invoked inside agent-box and
//! how what it left is read back. No process code: the driver launches the
//! invocation and hands the output in. [`contract`] is the suite every
//! adapter passes, run against the fake agent and the Claude adapter; the
//! Codex adapter (M4) is held to it too.

use serde::Serialize;
use serde_json::Value;

use crate::brief;
use crate::store::Agent;

/// The brief's path inside the box.
pub const BRIEF: &str = "/run/brief.json";

/// What an attempt is, as an adapter needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptSpec {
    pub unit: String,
    pub attempt: u32,
    pub model: String,
    pub runtime_bound: u64,
}

/// The agent's command line inside the box: `prompt` and then `args`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub prompt: String,
    pub args: Vec<String>,
}

/// What an attempt's run left, as the driver reads it: the unit's exit
/// status (`None` when it was killed by a signal or stopped), the agent's
/// stdout, and the commits on the attempt branch past the base, oldest
/// first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOutput {
    pub exit: Option<i32>,
    pub stdout: Vec<u8>,
    pub commits: Vec<String>,
}

/// Tokens and dollars, where the agent reports them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub dollars: f64,
}

/// What an attempt produced.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AttemptResult {
    pub commits: Vec<String>,
    pub exit: Option<i32>,
    /// The agent's events, where its output offers them.
    pub events: Option<Vec<Value>>,
    pub usage: Option<Usage>,
}

pub trait AgentAdapter {
    fn agent(&self) -> Agent;

    /// The command line for `spec`. The same spec always gives the same
    /// invocation, and it carries no credential.
    fn invocation(&self, spec: &AttemptSpec) -> Invocation;

    /// Reads the run's output. Never fails: output it cannot read leaves
    /// `events` and `usage` empty, and the exit and commits stand.
    fn result(&self, spec: &AttemptSpec, output: &RunOutput) -> AttemptResult;
}

/// The prompt every coder attempt starts with: the `implement` skill on the
/// brief.
fn implement() -> String {
    format!("/implement {BRIEF}")
}

/// Claude Code, run as `claude -p <prompt> --output-format json --model
/// <model>`. No `--bare` (Q17, ticket S-0001/6): it reads no OAuth sign-in
/// and skips the `CLAUDE.md` pointer.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn agent(&self) -> Agent {
        Agent::Claude
    }

    fn invocation(&self, spec: &AttemptSpec) -> Invocation {
        Invocation {
            prompt: implement(),
            args: vec![
                "--output-format".to_owned(),
                "json".to_owned(),
                "--model".to_owned(),
                spec.model.clone(),
            ],
        }
    }

    /// `--output-format json` prints one result object, with
    /// `total_cost_usd` and `usage`; it offers no events.
    fn result(&self, _spec: &AttemptSpec, output: &RunOutput) -> AttemptResult {
        let parsed: Option<Value> = serde_json::from_slice(&output.stdout).ok();
        let usage = parsed.as_ref().and_then(|v| {
            let tokens = v.get("usage")?;
            let count = |key: &str| tokens.get(key).and_then(Value::as_u64).unwrap_or(0);
            Some(Usage {
                input_tokens: count("input_tokens")
                    + count("cache_creation_input_tokens")
                    + count("cache_read_input_tokens"),
                output_tokens: count("output_tokens"),
                dollars: v.get("total_cost_usd")?.as_f64()?,
            })
        });
        AttemptResult {
            commits: output.commits.clone(),
            exit: output.exit,
            events: None,
            usage,
        }
    }
}

/// The fake agent's adapter: the same prompt, no options, and stdout read
/// as JSON lines, each an event; a line with `tokens` and `dollars` is the
/// usage. The scripted agent that runs it lives with the driver's tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeAdapter;

impl AgentAdapter for FakeAdapter {
    fn agent(&self) -> Agent {
        Agent::Claude
    }

    fn invocation(&self, _spec: &AttemptSpec) -> Invocation {
        Invocation {
            prompt: implement(),
            args: Vec::new(),
        }
    }

    fn result(&self, _spec: &AttemptSpec, output: &RunOutput) -> AttemptResult {
        let events: Vec<Value> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let usage = events.iter().find_map(|e| {
            Some(Usage {
                input_tokens: e.get("tokens")?.as_u64()?,
                output_tokens: 0,
                dollars: e.get("dollars")?.as_f64()?,
            })
        });
        AttemptResult {
            commits: output.commits.clone(),
            exit: output.exit,
            events: (!events.is_empty()).then_some(events),
            usage,
        }
    }
}

/// What a contract case asks the agent to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Script {
    pub commits: u32,
    /// Write a sealed outbox.
    pub outbox: bool,
    /// The exit status; `None` is killed.
    pub exit: Option<i32>,
}

/// Runs an invocation as the agent would, following `script`, and returns
/// what the driver would read back. The fake agent's runner does it in
/// process; the Claude adapter's runs a stand-in `claude`.
pub trait Runner {
    fn run(&mut self, invocation: &Invocation, script: &Script) -> RunOutput;
}

/// The contract suite. Every failure is returned, each naming its case.
pub fn contract(adapter: &dyn AgentAdapter, runner: &mut dyn Runner) -> Result<(), Vec<String>> {
    let mut failures = Vec::new();
    let mut fail = |case: &str, what: String| failures.push(format!("{case}: {what}"));
    let spec = AttemptSpec {
        unit: "gauge-run-1-1".to_owned(),
        attempt: 1,
        model: "a-model".to_owned(),
        runtime_bound: 60,
    };

    let invocation = adapter.invocation(&spec);
    if invocation != adapter.invocation(&spec) {
        fail(
            "invocation",
            "differs between two calls on one spec".to_owned(),
        );
    }
    if !invocation.prompt.contains(BRIEF) || !invocation.prompt.starts_with("/implement") {
        fail(
            "invocation",
            format!(
                "prompt {:?} does not run implement on the brief",
                invocation.prompt
            ),
        );
    }
    if std::iter::once(&invocation.prompt)
        .chain(&invocation.args)
        .any(|s| brief::credential(s))
    {
        fail(
            "invocation",
            "carries something that looks like a credential".to_owned(),
        );
    }

    let cases = [
        (
            "hand back",
            Script {
                commits: 2,
                outbox: true,
                exit: Some(0),
            },
        ),
        (
            "exit non-zero",
            Script {
                commits: 0,
                outbox: false,
                exit: Some(3),
            },
        ),
        (
            "commit, then exit non-zero",
            Script {
                commits: 1,
                outbox: false,
                exit: Some(1),
            },
        ),
        (
            "killed",
            Script {
                commits: 1,
                outbox: false,
                exit: None,
            },
        ),
    ];
    for (case, script) in cases {
        let output = runner.run(&invocation, &script);
        if output.commits.len() != script.commits as usize {
            fail(
                case,
                format!(
                    "the runner left {} commits, not {}",
                    output.commits.len(),
                    script.commits
                ),
            );
        }
        let result = adapter.result(&spec, &output);
        if result.exit != script.exit {
            fail(
                case,
                format!("exit {:?}, not {:?}", result.exit, script.exit),
            );
        }
        if result.commits != output.commits {
            fail(case, "the commits are not the run's".to_owned());
        }
        if let Some(usage) = result.usage
            && !(usage.dollars >= 0.0 && usage.dollars.is_finite())
        {
            fail(case, format!("dollars {} is not a cost", usage.dollars));
        }
        // Output the adapter cannot read changes neither exit nor commits.
        for stdout in [&b""[..], b"not json\n", b"{\"type\":"] {
            let garbled = RunOutput {
                stdout: stdout.to_vec(),
                ..output.clone()
            };
            let read = adapter.result(&spec, &garbled);
            if read.exit != output.exit || read.commits != output.commits || read.usage.is_some() {
                fail(
                    case,
                    format!("unreadable stdout {stdout:?} changed the result"),
                );
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}
