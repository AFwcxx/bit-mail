use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "bit-mail", version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser, error::ErrorKind};

    use super::Cli;

    #[test]
    fn planned_commands_are_not_claimed_before_they_are_implemented() {
        Cli::command().debug_assert();

        let error = Cli::try_parse_from(["bit-mail", "pull"])
            .expect_err("unimplemented commands must be rejected");

        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
}
