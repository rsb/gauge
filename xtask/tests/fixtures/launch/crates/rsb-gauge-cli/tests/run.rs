use std::process::Command;

#[test]
fn runs() {
    let _ = Command::new(env!("CARGO")).status();
}
