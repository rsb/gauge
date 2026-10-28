//! The clone behind [`GitHub::clone_at`](crate::world::GitHub::clone_at):
//! `git` run with the credential in its environment for the one command that
//! needs it, so it is never written to `.git/config` or anywhere on disk.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::Error;

/// A credential for one fetch: the header git sends, and the secret inside
/// it, which is cut from anything git prints before it reaches an error.
pub struct Auth {
    /// The URL prefix the header is sent to, as `http.<url>.extraHeader`.
    pub url: String,
    pub header: String,
    pub secret: String,
}

/// Fetches `sha` from `url` into a new repository at `into`, which must be
/// absent or empty, and checks it out detached. `origin` is set to `url`,
/// which must carry no credential: `auth` goes only into the environment of
/// the fetch.
pub fn fetch_at(url: &str, sha: &str, into: &Path, auth: Option<&Auth>) -> Result<(), Error> {
    if !is_sha(sha) {
        return Err(Error::Sha(sha.to_owned()));
    }
    let empty = match std::fs::read_dir(into) {
        Ok(mut entries) => entries.next().is_none(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(source) => {
            return Err(Error::Read {
                path: into.to_path_buf(),
                source,
            });
        }
    };
    if !empty {
        return Err(Error::CloneTarget(into.to_path_buf()));
    }
    std::fs::create_dir_all(into).map_err(|source| Error::Write {
        path: into.to_path_buf(),
        source,
    })?;
    let secret = auth.map(|a| a.secret.as_str());
    git(into, "init", &["init", "-q"], None, secret)?;
    git(
        into,
        "remote add",
        &["remote", "add", "origin", url],
        None,
        secret,
    )?;
    git(
        into,
        "fetch",
        &["fetch", "-q", "--no-tags", "origin", sha],
        auth,
        secret,
    )?;
    git(
        into,
        "checkout",
        &["checkout", "-q", "--detach", sha],
        None,
        secret,
    )?;
    Ok(())
}

/// A full commit id: 40 hex digits for SHA-1, 64 for SHA-256.
fn is_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Runs one `git` command in `dir`. Prompts are off, no credential helper is
/// asked or told anything, and tracing is off so no header is logged.
fn git(
    dir: &Path,
    what: &'static str,
    args: &[&str],
    auth: Option<&Auth>,
    secret: Option<&str>,
) -> Result<(), Error> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "");
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_TRACE") {
            command.env_remove(key);
        }
    }
    // An empty `credential.helper` clears every helper configured, so none
    // can store what this command uses.
    let mut config: Vec<(String, &str)> = vec![("credential.helper".to_owned(), "")];
    if let Some(auth) = auth {
        config.push((format!("http.{}.extraHeader", auth.url), &auth.header));
    }
    command.env("GIT_CONFIG_COUNT", config.len().to_string());
    for (n, (key, value)) in config.iter().enumerate() {
        command
            .env(format!("GIT_CONFIG_KEY_{n}"), key)
            .env(format!("GIT_CONFIG_VALUE_{n}"), OsStr::new(value));
    }
    let output = command.output().map_err(Error::GitStart)?;
    if output.status.success() {
        return Ok(());
    }
    let mut stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if let Some(secret) = secret.filter(|s| !s.is_empty()) {
        stderr = stderr.replace(secret, "<redacted>");
    }
    Err(Error::Git {
        what,
        status: output.status.code(),
        stderr,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A bare repository with one commit, and that commit's id.
    pub fn bare(dir: &Path) -> (std::path::PathBuf, String) {
        let work = dir.join("work");
        let bare = dir.join("origin.git");
        let run = |cwd: &Path, args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {out:?}");
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        };
        std::fs::create_dir_all(work.join(".rsb")).unwrap();
        std::fs::write(work.join(".rsb/governance.lock"), "[root]\n").unwrap();
        run(&work, &["init", "-q", "-b", "main"]);
        run(&work, &["add", "."]);
        run(&work, &["commit", "-q", "-m", "init"]);
        let sha = run(&work, &["rev-parse", "HEAD"]);
        run(dir, &["clone", "-q", "--bare", "work", "origin.git"]);
        (bare, sha)
    }

    const SECRET: &str = "ghs_SENTINEL0123456789";

    fn auth() -> Auth {
        Auth {
            url: "https://github.com/".into(),
            header: format!("AUTHORIZATION: basic {SECRET}"),
            secret: SECRET.into(),
        }
    }

    /// Every file under `dir`, read as bytes.
    fn files(dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
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
    fn fetches_the_sha_and_writes_the_credential_nowhere() {
        let dir = tempfile::tempdir().unwrap();
        let (bare, sha) = bare(dir.path());
        let url = format!("file://{}", bare.display());
        let into = dir.path().join("clone");
        fetch_at(&url, &sha, &into, Some(&auth())).unwrap();
        assert_eq!(
            std::fs::read_to_string(into.join(".rsb/governance.lock")).unwrap(),
            "[root]\n"
        );
        let head = std::fs::read_to_string(into.join(".git/HEAD")).unwrap();
        assert_eq!(head.trim(), sha);
        let config = std::fs::read_to_string(into.join(".git/config")).unwrap();
        assert!(config.contains(&url), "{config}");
        assert!(!config.to_lowercase().contains("extraheader"), "{config}");
        for (path, bytes) in files(&into) {
            assert!(
                !String::from_utf8_lossy(&bytes).contains(SECRET),
                "the credential is in {}",
                path.display()
            );
        }
    }

    #[test]
    fn a_failed_fetch_names_the_step_and_never_the_credential() {
        let dir = tempfile::tempdir().unwrap();
        let (bare, _) = bare(dir.path());
        let url = format!("file://{}", bare.display());
        let missing = "0".repeat(40);
        let err = fetch_at(&url, &missing, &dir.path().join("clone"), Some(&auth()))
            .err()
            .unwrap();
        let text = crate::error::report(&err);
        assert!(text.starts_with("git fetch exited"), "{text}");
        assert!(!text.contains(SECRET), "{text}");
    }

    #[test]
    fn refuses_a_target_that_is_not_empty_and_a_sha_that_is_not_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x"), "").unwrap();
        let sha = "a".repeat(40);
        assert!(matches!(
            fetch_at("file:///nowhere", &sha, dir.path(), None),
            Err(Error::CloneTarget(_))
        ));
        assert!(matches!(
            fetch_at("file:///nowhere", "main", &dir.path().join("c"), None),
            Err(Error::Sha(_))
        ));
        assert!(matches!(
            fetch_at(
                "file:///nowhere",
                "--upload-pack=x",
                &dir.path().join("c"),
                None
            ),
            Err(Error::Sha(_))
        ));
    }
}
