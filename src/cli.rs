use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "bit-mail", version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {
    /// Select an account by alias.
    #[arg(long, global = true)]
    pub account: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a runtime repository in the current directory.
    Init,
    /// Connect or reauthorize a Gmail account.
    Connect {
        /// Reauthorize an existing account alias.
        #[arg(long, value_name = "ALIAS")]
        reauthorize: Option<String>,
    },
    /// Read or update framework configuration.
    Config(ConfigArgs),
    /// List configured accounts.
    Accounts,
    /// Manage one account.
    Account(AccountArgs),
    /// Print the selected account's data path.
    Path(AccountScopeArgs),
    /// Pull provider truth into the local repository.
    Pull(PullArgs),
    /// Fetch an attachment on demand.
    Attachment(AttachmentArgs),
    /// Fetch optional raw message source.
    Raw(RawArgs),
    /// List actionable local work items.
    WorkItems(WorkItemsArgs),
    /// Stage local read/delete intent.
    Stage(StageArgs),
    /// Return staged local intent to pending.
    Unstage(UnstageArgs),
    /// Manage account-scoped selections.
    Selection(SelectionArgs),
    /// Manage approved repository Knowledge.
    Knowledge(KnowledgeArgs),
}

#[derive(Debug, Args)]
pub struct WorkItemsArgs {
    #[arg(long)]
    pub state: Option<crate::triage::WorkState>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct StageArgs {
    #[arg(value_name = "ID... ACTION", required = true)]
    pub values: Vec<String>,
    #[arg(long, conflicts_with = "selection")]
    pub stdin: bool,
    #[arg(long, value_name = "NAME", conflicts_with = "stdin")]
    pub selection: Option<String>,
}

#[derive(Debug, Args)]
pub struct UnstageArgs {
    #[arg(required_unless_present = "selection", conflicts_with = "selection")]
    pub ids: Vec<uuid::Uuid>,
    #[arg(long, value_name = "NAME")]
    pub selection: Option<String>,
}

#[derive(Debug, Args)]
pub struct SelectionArgs {
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: SelectionCommand,
}

#[derive(Debug, Subcommand)]
pub enum SelectionCommand {
    Create {
        name: String,
    },
    Add {
        name: String,
        #[arg(required = true)]
        ids: Vec<uuid::Uuid>,
    },
    Remove {
        name: String,
        #[arg(required = true)]
        ids: Vec<uuid::Uuid>,
    },
    Show {
        name: String,
    },
    Delete {
        name: String,
    },
}

#[derive(Debug, Args)]
pub struct KnowledgeArgs {
    #[command(subcommand)]
    pub command: KnowledgeCommand,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeCommand {
    Add {
        content: String,
    },
    List {
        #[arg(long)]
        json: bool,
    },
    Show {
        id: uuid::Uuid,
    },
    Update {
        id: uuid::Uuid,
        content: String,
    },
    Remove {
        id: uuid::Uuid,
    },
}

#[derive(Debug, Args)]
pub struct PullArgs {
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..), conflicts_with = "all")]
    pub limit: Option<u32>,
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub all_accounts: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct AttachmentArgs {
    #[command(subcommand)]
    pub command: AttachmentCommand,
}

#[derive(Debug, Subcommand)]
pub enum AttachmentCommand {
    Fetch {
        message_id: uuid::Uuid,
        part_id: String,
    },
}

#[derive(Debug, Args)]
pub struct RawArgs {
    #[command(subcommand)]
    pub command: RawCommand,
}

#[derive(Debug, Subcommand)]
pub enum RawCommand {
    Fetch { message_id: uuid::Uuid },
}

#[derive(Debug, Args)]
pub struct AccountScopeArgs {
    /// Select every configured account.
    #[arg(long)]
    pub all_accounts: bool,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show effective repository configuration.
    Show {
        /// Emit JSON instead of TOML.
        #[arg(long)]
        json: bool,
    },
    /// Set one supported configuration key.
    Set { key: String, value: String },
}

#[derive(Debug, Args)]
pub struct AccountArgs {
    #[command(subcommand)]
    pub command: AccountCommand,
}

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// Rename an account alias without moving its UUID-owned data.
    Rename {
        old_alias: String,
        new_alias: String,
    },
    /// Remove a local account binding conservatively.
    Remove {
        alias: String,
        /// Delete non-empty UUID-owned local state.
        #[arg(long)]
        discard_local_data: bool,
        /// Explicitly retain any external credential material.
        #[arg(long, conflicts_with = "revoke_credentials")]
        keep_credentials: bool,
        /// Revoke credential material before removing the binding.
        #[arg(long, conflicts_with = "keep_credentials")]
        revoke_credentials: bool,
    },
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser, error::ErrorKind};

    use super::Cli;

    #[test]
    fn pull_cli_rejects_conflicting_bounds() {
        Cli::command().debug_assert();
        let error = Cli::try_parse_from(["bit-mail", "pull", "--all", "--limit", "2"])
            .expect_err("bounds conflict");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn connect_and_reauthorize_have_stable_cli_shapes() {
        assert!(Cli::try_parse_from(["bit-mail", "connect"]).is_ok());
        assert!(Cli::try_parse_from(["bit-mail", "connect", "--reauthorize", "personal"]).is_ok());
    }

    #[test]
    fn offline_triage_commands_have_stable_cli_shapes() {
        for arguments in [
            vec!["bit-mail", "work-items", "--state", "pending", "--json"],
            vec!["bit-mail", "stage", "--stdin", "delete"],
            vec!["bit-mail", "stage", "--selection", "review", "read"],
            vec!["bit-mail", "unstage", "--selection", "review"],
            vec!["bit-mail", "selection", "show", "review", "--json"],
            vec!["bit-mail", "knowledge", "list", "--json"],
        ] {
            assert!(Cli::try_parse_from(arguments).is_ok());
        }
        Cli::command().debug_assert();
    }
}
