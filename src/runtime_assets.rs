pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

include!(concat!(env!("OUT_DIR"), "/runtime_assets.rs"));

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use crate::cli::Cli;

    use super::ASSETS;

    #[test]
    fn bundled_runtime_contains_bootstrap_and_all_four_skills() {
        let paths = ASSETS.iter().map(|(path, _)| *path).collect::<Vec<_>>();
        assert!(paths.contains(&"AGENTS.md"));
        for skill in [
            "bit-mail-core",
            "bulk-review",
            "inbox-triage",
            "knowledge-management",
        ] {
            assert!(
                paths
                    .iter()
                    .any(|path| *path == format!("skills/{skill}/SKILL.md"))
            );
        }
    }

    #[test]
    fn runtime_instructions_name_only_real_cli_commands_and_options() {
        let root = Cli::command();
        for (path, bytes) in ASSETS {
            let Ok(text) = std::str::from_utf8(bytes) else {
                continue;
            };
            for snippet in text.split('`').skip(1).step_by(2) {
                let Some(command_line) = snippet.strip_prefix("bit-mail ") else {
                    continue;
                };
                let mut command = &root;
                for name in command_line.split_whitespace() {
                    if name.contains("...") {
                        break;
                    }
                    if name.starts_with('<') {
                        continue;
                    }
                    if let Some(name) = name.strip_prefix("--") {
                        assert!(
                            command
                                .get_arguments()
                                .any(|argument| argument.get_long() == Some(name)),
                            "runtime asset {path} names unknown option `{name}` in `{snippet}`"
                        );
                        continue;
                    }
                    command = command.find_subcommand(name).unwrap_or_else(|| {
                        panic!("runtime asset {path} names unknown command `{snippet}`")
                    });
                }
            }
        }
    }
}
