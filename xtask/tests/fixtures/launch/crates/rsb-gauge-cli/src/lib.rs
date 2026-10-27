use std::process::ExitCode;

pub fn main() -> ExitCode {
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    #[test]
    fn runs() {
        let _ = std::process::Command::new("true").status();
    }
}
