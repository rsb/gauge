use std::process::Command;

pub fn fetch() {
    let _ = Command::new("curl").status();
}
