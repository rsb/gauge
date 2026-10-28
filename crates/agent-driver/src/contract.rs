//! The adapter contract suite with runners that act for real: commits in a
//! git repository, an outbox and manifest on disk, a real exit status. The
//! fake agent runs in process under [`FakeAdapter`]; the Claude adapter runs
//! a stand-in `claude` that prints the `--output-format json` result object.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use rsb_gauge_reconcile::adapter::{
    ClaudeAdapter, FakeAdapter, Invocation, RunOutput, Runner, Script, contract,
};

use crate::attempt::tests::sh_git;

/// A temporary attempt directory: `repo/` with one commit, `run/outbox/`.
struct Attempt {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    outbox: PathBuf,
}

impl Attempt {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let outbox = tmp.path().join("run/outbox");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&outbox).unwrap();
        std::fs::write(repo.join("README.md"), "base\n").unwrap();
        sh_git(&repo, &["init", "-q", "-b", "feat/1-x"]);
        sh_git(&repo, &["add", "."]);
        sh_git(&repo, &["commit", "-q", "-m", "base"]);
        Self {
            _tmp: tmp,
            repo,
            outbox,
        }
    }

    fn head(&self) -> String {
        sh_git(&self.repo, &["rev-parse", "HEAD"])
    }

    /// The commits past `base`, oldest first.
    fn commits(&self, base: &str) -> Vec<String> {
        sh_git(
            &self.repo,
            &["rev-list", "--reverse", &format!("{base}..HEAD")],
        )
        .lines()
        .map(str::to_owned)
        .collect()
    }

    /// An empty outbox for the next case.
    fn clear_outbox(&self) {
        std::fs::remove_dir_all(&self.outbox).unwrap();
        std::fs::create_dir_all(&self.outbox).unwrap();
    }

    fn sealed(&self) -> bool {
        self.outbox.join("manifest.json").is_file()
    }
}

/// The fake agent: commits, seals the outbox and prints its events as the
/// script says, in process.
struct FakeAgent {
    attempt: Attempt,
    runs: usize,
}

impl Runner for FakeAgent {
    fn run(&mut self, invocation: &Invocation, script: &Script) -> RunOutput {
        assert_eq!(invocation.prompt, "/implement /run/brief.json");
        self.runs += 1;
        let a = &self.attempt;
        a.clear_outbox();
        let base = a.head();
        for n in 0..script.commits {
            let file = format!("work-{}-{n}.txt", self.runs);
            std::fs::write(a.repo.join(&file), format!("{n}\n")).unwrap();
            sh_git(&a.repo, &["add", &file]);
            sh_git(&a.repo, &["commit", "-q", "-m", &format!("work {n}")]);
        }
        if script.outbox {
            std::fs::write(a.outbox.join("01-pr-open.json"), "{}\n").unwrap();
            std::fs::write(a.outbox.join("manifest.json"), "{\"entries\":1}\n").unwrap();
        }
        assert_eq!(a.sealed(), script.outbox);
        RunOutput {
            exit: script.exit,
            stdout: b"{\"step\":\"implement\"}\n{\"tokens\":120,\"dollars\":0.25}\n".to_vec(),
            commits: a.commits(&base),
        }
    }
}

#[test]
fn the_fake_agent_passes_the_contract_with_real_commits_and_outbox() {
    let mut runner = FakeAgent {
        attempt: Attempt::new(),
        runs: 0,
    };
    assert_eq!(contract(&FakeAdapter, &mut runner), Ok(()));
    assert_eq!(runner.runs, 4);
}

/// A stand-in `claude` on `PATH`: it checks its command line, commits and
/// seals the outbox as told, prints Claude's `--output-format json` result
/// object, and exits with the code, or kills itself.
const CLAUDE: &str = r#"#!/bin/sh
set -eu
[ "$1" = -p ] || { echo "no -p" >&2; exit 64; }
case " $* " in *" --output-format json "*) ;; *) echo "not json" >&2; exit 64 ;; esac
case " $* " in *" --bare "*) echo "--bare" >&2; exit 64 ;; esac
i=0
while [ "$i" -lt "$SCRIPT_COMMITS" ]; do
  i=$((i + 1))
  f="work-$SCRIPT_RUN-$i.txt"
  echo "$i" > "$f"
  git add "$f"
  git commit -q -m "work $i"
done
if [ "$SCRIPT_OUTBOX" = yes ]; then
  echo '{}' > "$OUTBOX/01-pr-open.json"
  echo '{"entries":1}' > "$OUTBOX/manifest.json"
fi
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"duration_ms":1200,"num_turns":3,"result":"done","session_id":"s","total_cost_usd":0.42,"usage":{"input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":30,"output_tokens":40}}'
if [ "$SCRIPT_EXIT" = kill ]; then
  kill -9 $$
fi
exit "$SCRIPT_EXIT"
"#;

struct StandIn {
    attempt: Attempt,
    bin: tempfile::TempDir,
    runs: usize,
}

impl StandIn {
    fn new() -> Self {
        let bin = tempfile::tempdir().unwrap();
        let claude = bin.path().join("claude");
        std::fs::write(&claude, CLAUDE).unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            attempt: Attempt::new(),
            bin,
            runs: 0,
        }
    }

    fn path(&self) -> String {
        let rest = std::env::var("PATH").unwrap_or_default();
        format!("{}:{rest}", self.bin.path().display())
    }
}

impl Runner for StandIn {
    fn run(&mut self, invocation: &Invocation, script: &Script) -> RunOutput {
        self.runs += 1;
        let a = &self.attempt;
        a.clear_outbox();
        let base = a.head();
        let output = Command::new("claude")
            .arg("-p")
            .arg(&invocation.prompt)
            .args(&invocation.args)
            .current_dir(&a.repo)
            .env("PATH", self.path())
            .env("SCRIPT_RUN", self.runs.to_string())
            .env("SCRIPT_COMMITS", script.commits.to_string())
            .env("SCRIPT_OUTBOX", if script.outbox { "yes" } else { "no" })
            .env("OUTBOX", &a.outbox)
            .env(
                "SCRIPT_EXIT",
                script.exit.map_or("kill".to_owned(), |c| c.to_string()),
            )
            .env("GIT_AUTHOR_NAME", "gauge-bot")
            .env("GIT_AUTHOR_EMAIL", "gauge-bot@example.com")
            .env("GIT_COMMITTER_NAME", "gauge-bot")
            .env("GIT_COMMITTER_EMAIL", "gauge-bot@example.com")
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("exit 64")
                && output.status.code() != Some(64),
            "{output:?}"
        );
        assert_eq!(a.sealed(), script.outbox);
        RunOutput {
            exit: output.status.code(),
            stdout: output.stdout,
            commits: a.commits(&base),
        }
    }
}

#[test]
fn the_claude_adapter_passes_the_contract_against_a_stand_in_claude() {
    let mut runner = StandIn::new();
    assert_eq!(contract(&ClaudeAdapter, &mut runner), Ok(()));
    assert_eq!(runner.runs, 4);
}

#[test]
fn the_claude_adapter_reads_the_stand_ins_cost_and_tokens() {
    use rsb_gauge_reconcile::adapter::{AgentAdapter, AttemptSpec};
    let spec = AttemptSpec {
        unit: "gauge-run-1-1".into(),
        attempt: 1,
        model: "claude-opus-5-5".into(),
        runtime_bound: 60,
    };
    let mut runner = StandIn::new();
    let output = runner.run(
        &ClaudeAdapter.invocation(&spec),
        &Script {
            commits: 1,
            outbox: true,
            exit: Some(0),
        },
    );
    let result = ClaudeAdapter.result(&spec, &output);
    assert_eq!(result.exit, Some(0));
    assert_eq!(result.commits.len(), 1);
    let usage = result.usage.unwrap();
    assert_eq!((usage.input_tokens, usage.output_tokens), (60, 40));
    assert_eq!(usage.dollars, 0.42);
    assert!(
        Path::new(&runner.attempt.outbox)
            .join("manifest.json")
            .is_file()
    );
}
