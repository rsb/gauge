//! The attempts port: an attempt's workspace on disk and its transient
//! systemd user unit, `gauge-run-<loop>-<attempt>`. The live adapter is
//! [`LiveAttempts`]; the fake, for the tick's tests and the kill harness, is
//! in `fake.rs`. Every call the tick makes here is one the kill harness can
//! die before.
//!
//! An attempt's directory is `<runs>/<loop>/<attempt>/`, with `repo/` (the
//! clone, on the attempt's branch) and `run/` (`brief.json`, `outbox/`, and
//! the unit's `agent.json` and `drive.log`, which the box does not mount).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rsb_gauge_reconcile::UnitSeen;
use rsb_gauge_reconcile::adapter::{Invocation, RunOutput};
use rsb_gauge_reconcile::store::{LoopId, SkillsSource};

use crate::config::AttemptConfig;
use crate::error::Error;
use crate::world::GitHub;

/// The skills agent-box mounts from the rsb-skills checkout.
pub const SKILLS: &[&str] = &[
    "skills/engineering/implement",
    "skills/engineering/tdd",
    "skills/engineering/rsb-code-review",
];

/// The `CLAUDE.md` written into a clone that has none: the one-line pointer
/// to the project's `AGENTS.md` (Q17).
pub const POINTER: &str = "@AGENTS.md\n";

/// Seconds systemd waits for the unit to stop before it kills it. Above
/// `drive.sh`'s 30 s grace for the container, so the box can save a
/// refreshed sign-in.
pub const STOP_TIMEOUT: u64 = 90;
const _: () = assert!(STOP_TIMEOUT > 30, "past drive.sh's grace");

/// Where attempts live: `$XDG_DATA_HOME/gauge/runs`, or
/// `~/.local/share/gauge/runs`. Never under the store's
/// `~/.local/state/gauge`.
pub fn runs_root(
    xdg_data_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, Error> {
    let base = match xdg_data_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(home.ok_or(Error::NoHome)?)
            .join(".local")
            .join("share"),
    };
    Ok(base.join("gauge").join("runs"))
}

/// What the tick knows of how attempts run: the `[attempt]` config, and
/// the `PATH` and `HOME` the unit runs with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub config: AttemptConfig,
    pub path: String,
    pub home: String,
}

/// The repo an attempt's workspace is cloned from, and its ticket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Workspace<'a> {
    pub repo: &'a str,
    pub base_sha: &'a str,
    /// From the repo's root.
    pub ticket_path: &'a str,
}

/// One unit to start: `drive.sh <unit> <attempt dir> <prompt> <args...>`
/// in agent-box, with exactly `env` set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub unit: String,
    pub dir: PathBuf,
    pub agent_box: PathBuf,
    pub prompt: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl Launch {
    /// The launch of `invocation` for the attempt in `dir`. The environment
    /// is the bot identity, the skills checkout, `PATH` and `HOME`: no
    /// credential. agent-box mounts the agent's own sign-in itself.
    pub fn new(
        settings: &Settings,
        unit: &str,
        dir: &Path,
        invocation: &Invocation,
    ) -> Result<Self, Error> {
        let config = &settings.config;
        let (Some(name), Some(email)) = (&config.agent_name, &config.agent_email) else {
            return Err(Error::NoAgentIdentity);
        };
        Ok(Self {
            unit: unit.to_owned(),
            dir: dir.to_path_buf(),
            agent_box: config.agent_box.clone(),
            prompt: invocation.prompt.clone(),
            args: invocation.args.clone(),
            env: vec![
                ("AGENT_NAME".to_owned(), name.clone()),
                ("AGENT_EMAIL".to_owned(), email.clone()),
                (
                    "RSB_SKILLS".to_owned(),
                    config.rsb_skills.display().to_string(),
                ),
                ("PATH".to_owned(), settings.path.clone()),
                ("HOME".to_owned(), settings.home.clone()),
            ],
        })
    }

    /// The `systemd-run` command line that starts the unit.
    pub fn argv(&self) -> Vec<String> {
        let run = self.dir.join("run");
        let mut argv: Vec<String> = [
            "systemd-run",
            "--user",
            "--unit",
            &self.unit,
            "-p",
            "RemainAfterExit=yes",
            "-p",
            &format!("TimeoutStopSec={STOP_TIMEOUT}"),
            "-p",
            &format!("StandardOutput=file:{}", run.join("agent.json").display()),
            "-p",
            &format!("StandardError=file:{}", run.join("drive.log").display()),
            "--working-directory",
            &self.agent_box.display().to_string(),
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        for (key, value) in &self.env {
            argv.push("--setenv".to_owned());
            argv.push(format!("{key}={value}"));
        }
        argv.push("--".to_owned());
        argv.push(self.agent_box.join("drive.sh").display().to_string());
        argv.push(self.unit.clone());
        argv.push(self.dir.display().to_string());
        argv.push(self.prompt.clone());
        argv.extend(self.args.iter().cloned());
        argv
    }
}

/// Attempts' workspaces and units, as the tick sees them.
pub trait Attempts {
    /// The attempt's directory. Pure: no call is made.
    fn dir(&self, loop_id: LoopId, attempt: u32) -> PathBuf;

    /// The unit named `name`, as systemd shows it.
    fn unit(&mut self, name: &str) -> Result<UnitSeen, Error>;

    /// Starts the unit. Fails when a unit of that name is loaded.
    fn start(&mut self, launch: &Launch) -> Result<(), Error>;

    /// Stops the unit; a unit that is not loaded is already stopped.
    fn stop(&mut self, name: &str) -> Result<(), Error>;

    /// Clones `ws.repo` at its base into `dir/repo` unless it is there, and
    /// reads the ticket file from it: `None` when the clone has none.
    fn ticket(
        &mut self,
        github: &mut dyn GitHub,
        dir: &Path,
        ws: &Workspace<'_>,
    ) -> Result<Option<String>, Error>;

    /// Makes the workspace ready for the unit, each step a no-op when done:
    /// the clone, `branch` checked out (created at the base if new, never
    /// pushed), the `CLAUDE.md` pointer if the repo has none, an empty
    /// `run/outbox/`, and `run/brief.json` written from `brief`.
    fn prepare(
        &mut self,
        github: &mut dyn GitHub,
        dir: &Path,
        ws: &Workspace<'_>,
        branch: &str,
        brief: &serde_json::Value,
    ) -> Result<(), Error>;

    /// Whether `run/outbox/manifest.json` exists: the outbox is sealed.
    fn sealed(&mut self, dir: &Path) -> Result<bool, Error>;

    /// What the run left: the unit's `exit`, its stdout, and the commits
    /// `base..branch`, oldest first.
    fn output(
        &mut self,
        dir: &Path,
        base_sha: &str,
        branch: &str,
        exit: Option<i32>,
    ) -> Result<RunOutput, Error>;

    /// The skills checkout's HEAD, and whether the mounted skills match it.
    fn skills(&mut self, checkout: &Path) -> Result<SkillsSource, Error>;
}

/// Reads `systemctl show -p LoadState,ActiveState,SubState,ExecMainCode,
/// ExecMainStatus` into what the tick sees. `ExecMainCode` is the `CLD_*`
/// code: 1 exited, 2 killed, 3 dumped; 0 before the main process ends.
pub fn parse_unit(unit: &str, show: &str) -> Result<UnitSeen, Error> {
    let field = |key: &str| {
        show.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
            .unwrap_or_default()
    };
    let (load, active, sub) = (field("LoadState"), field("ActiveState"), field("SubState"));
    let code: u32 = field("ExecMainCode").parse().unwrap_or(0);
    let status: Option<i32> = field("ExecMainStatus").parse().ok();
    let exited = || UnitSeen::Exited {
        code: if code == 1 { status } else { None },
    };
    let unknown = || Error::UnitState {
        unit: unit.to_owned(),
        state: format!("{load} {active} {sub}"),
    };
    if load == "not-found" {
        return Ok(UnitSeen::Absent);
    }
    Ok(match active {
        "active" if sub == "exited" => exited(),
        "active" | "activating" | "deactivating" | "reloading" | "refreshing" => UnitSeen::Running,
        "failed" => exited(),
        "inactive" if code != 0 => exited(),
        "inactive" => UnitSeen::Absent,
        _ => return Err(unknown()),
    })
}

/// The live attempts: workspaces under `root`, units on the user's systemd.
pub struct LiveAttempts {
    root: PathBuf,
}

impl LiveAttempts {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

/// Runs `program args` in `dir`, prompts off, and returns its stdout, or
/// its status and stderr when it fails.
fn run(program: &'static str, dir: Option<&Path>, args: &[&str]) -> Result<String, Error> {
    let mut command = Command::new(program);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let output = command
        .args(args)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| Error::Start(program, e))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(Error::Command {
        what: format!("{program} {}", args.first().copied().unwrap_or_default()),
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn git(dir: &Path, args: &[&str]) -> Result<String, Error> {
    run("git", Some(dir), args)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let fail = |source| Error::Write {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(fail)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(fail)
}

/// Clones the repo into `dir/repo` unless a clone holding the base is
/// there. A clone a killed tick left half made is removed and made again.
fn clone(github: &mut dyn GitHub, dir: &Path, ws: &Workspace<'_>) -> Result<PathBuf, Error> {
    let repo = dir.join("repo");
    if repo.join(".git").is_dir() {
        let commit = format!("{}^{{commit}}", ws.base_sha);
        if git(&repo, &["rev-parse", "-q", "--verify", &commit]).is_ok() {
            return Ok(repo);
        }
    }
    if repo.exists() {
        std::fs::remove_dir_all(&repo).map_err(|source| Error::Write {
            path: repo.clone(),
            source,
        })?;
    }
    github.clone_at(ws.repo, ws.base_sha, &repo)?;
    Ok(repo)
}

impl Attempts for LiveAttempts {
    fn dir(&self, loop_id: LoopId, attempt: u32) -> PathBuf {
        self.root
            .join(loop_id.to_string())
            .join(attempt.to_string())
    }

    fn unit(&mut self, name: &str) -> Result<UnitSeen, Error> {
        let show = run(
            "systemctl",
            None,
            &[
                "--user",
                "show",
                &format!("{name}.service"),
                "-p",
                "LoadState,ActiveState,SubState,ExecMainCode,ExecMainStatus",
            ],
        )?;
        parse_unit(name, &show)
    }

    fn start(&mut self, launch: &Launch) -> Result<(), Error> {
        let argv = launch.argv();
        let args: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
        run("systemd-run", None, &args).map(|_| ())
    }

    fn stop(&mut self, name: &str) -> Result<(), Error> {
        match run(
            "systemctl",
            None,
            &["--user", "stop", &format!("{name}.service")],
        ) {
            // 5: the unit is not loaded, so it is stopped already.
            Err(Error::Command {
                status: Some(5), ..
            }) => Ok(()),
            other => other.map(|_| ()),
        }
    }

    fn ticket(
        &mut self,
        github: &mut dyn GitHub,
        dir: &Path,
        ws: &Workspace<'_>,
    ) -> Result<Option<String>, Error> {
        let repo = clone(github, dir, ws)?;
        let path = repo.join(ws.ticket_path);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::Read { path, source }),
        }
    }

    fn prepare(
        &mut self,
        github: &mut dyn GitHub,
        dir: &Path,
        ws: &Workspace<'_>,
        branch: &str,
        brief: &serde_json::Value,
    ) -> Result<(), Error> {
        let repo = clone(github, dir, ws)?;
        let head = format!("refs/heads/{branch}");
        if git(&repo, &["rev-parse", "-q", "--verify", &head]).is_ok() {
            git(&repo, &["checkout", "-q", branch])?;
        } else {
            git(&repo, &["checkout", "-q", "-b", branch, ws.base_sha])?;
        }
        let tracked = git(&repo, &["ls-files", "--", "CLAUDE.md"])?;
        if tracked.trim().is_empty() {
            let pointer = repo.join("CLAUDE.md");
            if !pointer.exists() {
                write(&pointer, POINTER.as_bytes())?;
            }
            let exclude = repo.join(".git/info/exclude");
            let mut text = match std::fs::read_to_string(&exclude) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(source) => {
                    return Err(Error::Read {
                        path: exclude,
                        source,
                    });
                }
            };
            if !text.lines().any(|l| l == "/CLAUDE.md") {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str("/CLAUDE.md\n");
                write(&exclude, text.as_bytes())?;
            }
        }
        let outbox = dir.join("run/outbox");
        std::fs::create_dir_all(&outbox).map_err(|source| Error::Write {
            path: outbox,
            source,
        })?;
        let mut bytes =
            serde_json::to_vec_pretty(brief).map_err(|e| Error::Failed(e.to_string()))?;
        bytes.push(b'\n');
        write(&dir.join("run/brief.json"), &bytes)
    }

    fn sealed(&mut self, dir: &Path) -> Result<bool, Error> {
        Ok(dir.join("run/outbox/manifest.json").is_file())
    }

    fn output(
        &mut self,
        dir: &Path,
        base_sha: &str,
        branch: &str,
        exit: Option<i32>,
    ) -> Result<RunOutput, Error> {
        let path = dir.join("run/agent.json");
        let stdout = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(source) => return Err(Error::Read { path, source }),
        };
        let repo = dir.join("repo");
        let head = format!("refs/heads/{branch}");
        let commits = if git(&repo, &["rev-parse", "-q", "--verify", &head]).is_ok() {
            git(
                &repo,
                &["rev-list", "--reverse", &format!("{base_sha}..{head}")],
            )?
            .lines()
            .map(str::to_owned)
            .collect()
        } else {
            Vec::new()
        };
        Ok(RunOutput {
            exit,
            stdout,
            commits,
        })
    }

    fn skills(&mut self, checkout: &Path) -> Result<SkillsSource, Error> {
        let commit = git(checkout, &["rev-parse", "HEAD"])?.trim().to_owned();
        let mut args = vec!["status", "--porcelain", "--"];
        args.extend(SKILLS);
        let status = git(checkout, &args)?;
        Ok(SkillsSource {
            commit,
            clean: status.trim().is_empty(),
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::fake::FakeGitHub;
    use rsb_gauge_reconcile::adapter::{AgentAdapter, AttemptSpec, ClaudeAdapter};

    pub fn settings() -> Settings {
        let mut config = AttemptConfig::defaults(Path::new("/home/u"));
        config.agent_name = Some("gauge-bot".into());
        config.agent_email = Some("gauge-bot@example.com".into());
        Settings {
            config,
            path: "/usr/bin:/bin".into(),
            home: "/home/u".into(),
        }
    }

    #[test]
    fn runs_live_under_the_data_home_never_the_state_dir() {
        let root = runs_root(None, Some("/home/u".into())).unwrap();
        assert_eq!(root, Path::new("/home/u/.local/share/gauge/runs"));
        let root = runs_root(Some("/data".into()), Some("/home/u".into())).unwrap();
        assert_eq!(root, Path::new("/data/gauge/runs"));
        let root = runs_root(Some("".into()), Some("/home/u".into())).unwrap();
        assert_eq!(root, Path::new("/home/u/.local/share/gauge/runs"));
        assert!(runs_root(None, None).is_err());
        let live = LiveAttempts::new(root);
        assert_eq!(
            live.dir(LoopId(4), 2),
            Path::new("/home/u/.local/share/gauge/runs/4/2")
        );
    }

    #[test]
    fn the_systemd_run_command_line_names_the_unit_and_carries_no_credential() {
        let spec = AttemptSpec {
            unit: "gauge-run-3-1".into(),
            attempt: 1,
            model: "claude-opus-5-5".into(),
            runtime_bound: 3600,
        };
        let dir = Path::new("/home/u/.local/share/gauge/runs/3/1");
        let launch = Launch::new(
            &settings(),
            "gauge-run-3-1",
            dir,
            &ClaudeAdapter.invocation(&spec),
        )
        .unwrap();
        let argv = launch.argv();
        let box_ = "/home/u/code/software/rsb/factory/agent-box";
        let expected: Vec<String> = [
            "systemd-run",
            "--user",
            "--unit",
            "gauge-run-3-1",
            "-p",
            "RemainAfterExit=yes",
            "-p",
            "TimeoutStopSec=90",
            "-p",
            "StandardOutput=file:/home/u/.local/share/gauge/runs/3/1/run/agent.json",
            "-p",
            "StandardError=file:/home/u/.local/share/gauge/runs/3/1/run/drive.log",
            "--working-directory",
            box_,
            "--setenv",
            "AGENT_NAME=gauge-bot",
            "--setenv",
            "AGENT_EMAIL=gauge-bot@example.com",
            "--setenv",
            "RSB_SKILLS=/home/u/code/software/rsb/factory/rsb-skills",
            "--setenv",
            "PATH=/usr/bin:/bin",
            "--setenv",
            "HOME=/home/u",
            "--",
            &format!("{box_}/drive.sh"),
            "gauge-run-3-1",
            "/home/u/.local/share/gauge/runs/3/1",
            "/implement /run/brief.json",
            "--output-format",
            "json",
            "--model",
            "claude-opus-5-5",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        assert_eq!(argv, expected);
        assert!(!argv.iter().any(|a| a == "--bare"), "Q17");
        assert!(
            !argv
                .iter()
                .any(|a| rsb_gauge_reconcile::brief::credential(a))
        );

        let mut anonymous = settings();
        anonymous.config.agent_email = None;
        assert!(matches!(
            Launch::new(&anonymous, "u", dir, &ClaudeAdapter.invocation(&spec)),
            Err(Error::NoAgentIdentity)
        ));
    }

    #[test]
    fn systemctl_show_reads_as_the_unit_the_tick_sees() {
        let show = |load, active, sub, code, status| {
            parse_unit(
                "gauge-run-1-1",
                &format!(
                    "LoadState={load}\nActiveState={active}\nSubState={sub}\n\
                     ExecMainCode={code}\nExecMainStatus={status}\n"
                ),
            )
            .unwrap()
        };
        assert_eq!(
            show("not-found", "inactive", "dead", 0, 0),
            UnitSeen::Absent
        );
        assert_eq!(show("loaded", "inactive", "dead", 0, 0), UnitSeen::Absent);
        for (active, sub) in [
            ("active", "running"),
            ("activating", "start"),
            ("deactivating", "stop-sigterm"),
        ] {
            assert_eq!(show("loaded", active, sub, 0, 0), UnitSeen::Running);
        }
        assert_eq!(
            show("loaded", "active", "exited", 1, 0),
            UnitSeen::Exited { code: Some(0) }
        );
        assert_eq!(
            show("loaded", "failed", "failed", 1, 3),
            UnitSeen::Exited { code: Some(3) }
        );
        assert_eq!(
            show("loaded", "failed", "failed", 2, 15),
            UnitSeen::Exited { code: None }
        );
        assert_eq!(
            show("loaded", "failed", "failed", 3, 6),
            UnitSeen::Exited { code: None }
        );
        assert_eq!(
            show("loaded", "inactive", "dead", 1, 0),
            UnitSeen::Exited { code: Some(0) }
        );
        assert!(parse_unit("u", "LoadState=loaded\nActiveState=odd\n").is_err());
    }

    /// Runs git in `dir`, as a test author.
    pub fn sh_git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    pub const TICKET: &str = "specs/S-0001-thin-loop/06-first-attempt.md";

    /// A bare repository holding the lockfile, `AGENTS.md` and a feature
    /// ticket, and its commit.
    pub fn bare_with_ticket(dir: &Path) -> (PathBuf, String) {
        let work = dir.join("work");
        std::fs::create_dir_all(work.join(".rsb")).unwrap();
        std::fs::create_dir_all(work.join("specs/S-0001-thin-loop")).unwrap();
        std::fs::write(work.join(".rsb/governance.lock"), "[root]\n").unwrap();
        std::fs::write(work.join("AGENTS.md"), "# AGENTS.md\n").unwrap();
        std::fs::write(
            work.join(TICKET),
            "---\nid: S-0001/6\nslice: feature\n---\n\n# First attempt\n",
        )
        .unwrap();
        sh_git(&work, &["init", "-q", "-b", "main"]);
        sh_git(&work, &["add", "."]);
        sh_git(&work, &["commit", "-q", "-m", "init"]);
        let sha = sh_git(&work, &["rev-parse", "HEAD"]);
        sh_git(dir, &["clone", "-q", "--bare", "work", "origin.git"]);
        (dir.join("origin.git"), sha)
    }

    /// Every file under `dir`, read as bytes.
    fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                out.extend(files(&path));
            } else {
                out.push((path.clone(), std::fs::read(&path).unwrap()));
            }
        }
        out
    }

    #[test]
    fn prepares_the_workspace_from_a_bare_repo_and_leaves_no_token() {
        let tmp = tempfile::tempdir().unwrap();
        let (bare, sha) = bare_with_ticket(tmp.path());
        let mut github = FakeGitHub::new().repo("lumen", |r| r.bare(&bare, &sha));
        let mut live = LiveAttempts::new(tmp.path().join("runs"));
        let dir = live.dir(LoopId(1), 1);
        let ws = Workspace {
            repo: "lumen",
            base_sha: &sha,
            ticket_path: TICKET,
        };
        let ticket = live.ticket(&mut github, &dir, &ws).unwrap().unwrap();
        let slice = rsb_gauge_reconcile::brief::slice(&ticket).unwrap();
        let branch = rsb_gauge_reconcile::brief::branch(slice, 7, TICKET).unwrap();
        assert_eq!(branch, "feat/7-first-attempt");
        let brief = serde_json::json!({ "schema": "brief/v1", "branch": branch });
        // Twice: every step is a no-op the second time.
        for _ in 0..2 {
            live.prepare(&mut github, &dir, &ws, &branch, &brief)
                .unwrap();
        }
        let repo = dir.join("repo");
        assert_eq!(
            sh_git(&repo, &["symbolic-ref", "--short", "HEAD"]),
            "feat/7-first-attempt"
        );
        assert_eq!(sh_git(&repo, &["rev-parse", "HEAD"]), sha);
        assert_eq!(
            std::fs::read_to_string(repo.join("CLAUDE.md")).unwrap(),
            "@AGENTS.md\n"
        );
        assert_eq!(
            sh_git(&repo, &["status", "--porcelain"]),
            "",
            "CLAUDE.md is not excluded"
        );
        let exclude = std::fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert_eq!(exclude.matches("/CLAUDE.md").count(), 1);
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("run/brief.json")).unwrap()).unwrap();
        assert_eq!(written, brief);
        assert_eq!(
            std::fs::read_dir(dir.join("run/outbox")).unwrap().count(),
            0
        );
        assert!(!live.sealed(&dir).unwrap());
        for (path, bytes) in files(&dir) {
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !rsb_gauge_reconcile::brief::credential(&text)
                    && !text.contains("x-access-token")
                    && !text.to_lowercase().contains("extraheader"),
                "a credential in {}",
                path.display()
            );
        }

        // The run's output: the commits past the base on the branch.
        sh_git(&repo, &["commit", "-q", "--allow-empty", "-m", "work"]);
        std::fs::write(dir.join("run/agent.json"), "{}").unwrap();
        let output = live.output(&dir, &sha, &branch, Some(1)).unwrap();
        assert_eq!(output.exit, Some(1));
        assert_eq!(output.stdout, b"{}");
        assert_eq!(output.commits, [sh_git(&repo, &["rev-parse", "HEAD"])]);
        std::fs::write(dir.join("run/outbox/manifest.json"), "{}").unwrap();
        assert!(live.sealed(&dir).unwrap());
    }

    #[test]
    fn a_repo_with_its_own_claude_md_keeps_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (bare, _) = bare_with_ticket(tmp.path());
        let work = tmp.path().join("work");
        std::fs::write(work.join("CLAUDE.md"), "# ours\n").unwrap();
        sh_git(&work, &["add", "."]);
        sh_git(&work, &["commit", "-q", "-m", "claude"]);
        sh_git(&work, &["push", "-q", bare.to_str().unwrap(), "main"]);
        let sha = sh_git(&work, &["rev-parse", "HEAD"]);
        let mut github = FakeGitHub::new().repo("lumen", |r| r.bare(&bare, &sha));
        let mut live = LiveAttempts::new(tmp.path().join("runs"));
        let dir = live.dir(LoopId(2), 1);
        let ws = Workspace {
            repo: "lumen",
            base_sha: &sha,
            ticket_path: TICKET,
        };
        live.prepare(&mut github, &dir, &ws, "feat/7-x", &serde_json::json!({}))
            .unwrap();
        let repo = dir.join("repo");
        assert_eq!(
            std::fs::read_to_string(repo.join("CLAUDE.md")).unwrap(),
            "# ours\n"
        );
        let exclude = std::fs::read_to_string(repo.join(".git/info/exclude")).unwrap_or_default();
        assert!(!exclude.contains("/CLAUDE.md"));
    }

    #[test]
    fn the_skills_source_is_the_head_and_whether_the_skills_are_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let skills = tmp.path().join("rsb-skills");
        for s in SKILLS {
            std::fs::create_dir_all(skills.join(s)).unwrap();
            std::fs::write(skills.join(s).join("SKILL.md"), "x\n").unwrap();
        }
        sh_git(&skills, &["init", "-q", "-b", "main"]);
        sh_git(&skills, &["add", "."]);
        sh_git(&skills, &["commit", "-q", "-m", "skills"]);
        let head = sh_git(&skills, &["rev-parse", "HEAD"]);
        let mut live = LiveAttempts::new(tmp.path().join("runs"));
        assert_eq!(
            live.skills(&skills).unwrap(),
            SkillsSource {
                commit: head.clone(),
                clean: true
            }
        );
        std::fs::write(skills.join("README.md"), "elsewhere\n").unwrap();
        assert!(live.skills(&skills).unwrap().clean, "outside the skills");
        std::fs::write(skills.join(SKILLS[1]).join("SKILL.md"), "changed\n").unwrap();
        assert_eq!(
            live.skills(&skills).unwrap(),
            SkillsSource {
                commit: head,
                clean: false
            }
        );
    }
}
