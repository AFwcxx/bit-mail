use std::{env, process::ExitCode};

use bit_mail::{
    Result,
    cli::{AccountCommand, AttachmentCommand, Cli, Command, ConfigCommand, RawCommand},
    credentials::{GoogleCredentialRevoker, KeyringStore},
    pull::{AccountReport, PullReport},
    repository::{AccountConfig, GitIgnorePolicy, RemoveOptions, Repository},
};
use clap::{CommandFactory, Parser};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn pull_accounts(
    accounts: Vec<AccountConfig>,
    mut pull: impl FnMut(&AccountConfig) -> Result<AccountReport>,
) -> PullReport {
    PullReport::new(
        accounts
            .iter()
            .map(|account| match pull(account) {
                Ok(report) => report,
                Err(error) => {
                    eprintln!("error: pull failed for {}: {error}", account.alias);
                    bit_mail::pull::failed_account_report(account)
                }
            })
            .collect(),
    )
}

fn run() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init()?;
    let cli = Cli::parse();
    match cli.command {
        None => {
            Cli::command().print_help()?;
            println!();
        }
        Some(Command::Init) => {
            let repository = Repository::initialize(&env::current_dir()?, GitIgnorePolicy::Prompt)?;
            println!(
                "Initialized bit-mail repository {} at {}",
                repository.id(),
                repository.root().display()
            );
        }
        Some(Command::Connect { reauthorize }) => {
            let repository = Repository::discover_current()?;
            bit_mail::connect::run(&repository, reauthorize.as_deref())?;
        }
        Some(Command::Config(args)) => {
            let repository = Repository::discover_current()?;
            match args.command {
                ConfigCommand::Show { json: true } => println!("{}", repository.config_json()?),
                ConfigCommand::Show { json: false } => print!("{}", repository.config_toml()?),
                ConfigCommand::Set { key, value } => {
                    repository.set_config(&key, &value)?;
                    println!("Updated {key}");
                }
            }
        }
        Some(Command::Accounts) => {
            for account in Repository::discover_current()?.accounts()? {
                println!("{}\t{}\t{}", account.alias, account.id, account.provider);
            }
        }
        Some(Command::Account(args)) => {
            let repository = Repository::discover_current()?;
            match args.command {
                AccountCommand::Rename {
                    old_alias,
                    new_alias,
                } => {
                    let account = repository.rename_account(&old_alias, &new_alias)?;
                    println!("Renamed account to {} ({})", account.alias, account.id);
                }
                AccountCommand::Remove {
                    alias,
                    discard_local_data,
                    keep_credentials,
                    revoke_credentials,
                } => {
                    let store = KeyringStore::new(repository.id());
                    repository.remove_account(
                        &alias,
                        RemoveOptions {
                            discard_local_data,
                            keep_credentials,
                            revoke_credentials,
                        },
                        &GoogleCredentialRevoker { store: &store },
                    )?;
                    println!("Removed account {alias}");
                }
            }
        }
        Some(Command::Path(args)) => {
            let repository = Repository::discover_current()?;
            if args.all_accounts {
                if cli.account.is_some() {
                    return Err(std::io::Error::other(
                        "--account cannot be used with --all-accounts",
                    )
                    .into());
                }
                for account in repository.accounts()? {
                    println!(
                        "{}\t{}",
                        account.alias,
                        repository.data_dir(account.id).display()
                    );
                }
            } else {
                let account = repository.resolve_account(
                    cli.account.as_deref(),
                    &env::current_dir()?,
                    env::var("BIT_MAIL_ACCOUNT").ok().as_deref(),
                )?;
                println!("{}", repository.data_dir(account.id).display());
            }
        }
        Some(Command::Pull(args)) => {
            let repository = Repository::discover_current()?;
            if args.all_accounts && cli.account.is_some() {
                return Err(
                    std::io::Error::other("--account cannot be used with --all-accounts").into(),
                );
            }
            let accounts = if args.all_accounts {
                repository.accounts()?
            } else {
                vec![repository.resolve_account(
                    cli.account.as_deref(),
                    &env::current_dir()?,
                    env::var("BIT_MAIL_ACCOUNT").ok().as_deref(),
                )?]
            };
            let options = bit_mail::pull::PullOptions {
                limit: args
                    .limit
                    .unwrap_or(repository.config()?.pull.default_limit),
                all: args.all,
            };
            let store = KeyringStore::new(repository.id());
            let report = pull_accounts(accounts, |account| {
                bit_mail::pull::pull_account(&repository, account, options, || {
                    Ok(Box::new(bit_mail::gmail::authorized_client(
                        &repository,
                        account,
                        &store,
                    )?))
                })
            });
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for account in &report.accounts {
                    let retries = account
                        .retries
                        .map_or_else(|| "unknown".into(), |value| value.to_string());
                    let backlog = account.backlog_remaining.map_or("unknown", |remaining| {
                        if remaining { "remaining" } else { "clear" }
                    });
                    println!(
                        "{}: {:?}; {} seeds, {} threads, {} additional unread, {} new/{} removed work items, {} retries, {} failures, backlog {}",
                        account.alias,
                        account.outcome,
                        account.seeds,
                        account.threads,
                        account.additional_unread,
                        account.new_work_items,
                        account.removed_work_items,
                        retries,
                        account.failures,
                        backlog
                    );
                }
            }
            if report.failed() {
                return Err(std::io::Error::other(
                    "pull completed with blocked or failed accounts",
                )
                .into());
            }
        }
        Some(Command::Attachment(args)) => {
            let repository = Repository::discover_current()?;
            let account = repository.resolve_account(
                cli.account.as_deref(),
                &env::current_dir()?,
                env::var("BIT_MAIL_ACCOUNT").ok().as_deref(),
            )?;
            let store = KeyringStore::new(repository.id());
            let AttachmentCommand::Fetch {
                message_id,
                part_id,
            } = args.command;
            let path = bit_mail::pull::fetch_attachment(
                &repository,
                &account,
                message_id,
                &part_id,
                || {
                    Ok(Box::new(bit_mail::gmail::authorized_client(
                        &repository,
                        &account,
                        &store,
                    )?))
                },
            )?;
            println!("{}", path.display());
        }
        Some(Command::Raw(args)) => {
            let repository = Repository::discover_current()?;
            let account = repository.resolve_account(
                cli.account.as_deref(),
                &env::current_dir()?,
                env::var("BIT_MAIL_ACCOUNT").ok().as_deref(),
            )?;
            let store = KeyringStore::new(repository.id());
            let RawCommand::Fetch { message_id } = args.command;
            let path = bit_mail::pull::fetch_raw(&repository, &account, message_id, || {
                Ok(Box::new(bit_mail::gmail::authorized_client(
                    &repository,
                    &account,
                    &store,
                )?))
            })?;
            println!("{}", path.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bit_mail::pull::Outcome;
    use uuid::Uuid;

    fn account(alias: &str) -> AccountConfig {
        AccountConfig {
            schema_version: 1,
            id: Uuid::new_v4(),
            alias: alias.into(),
            provider: "gmail".into(),
            provider_identity: None,
            credential_profile: None,
        }
    }

    #[test]
    fn blocked_account_does_not_stop_the_next_account() {
        let accounts = vec![account("blocked"), account("clean")];
        let mut visited = Vec::new();
        let report = pull_accounts(accounts, |account| {
            visited.push(account.alias.clone());
            let mut value = bit_mail::pull::failed_account_report(account);
            value.outcome = if account.alias == "blocked" {
                Outcome::Blocked
            } else {
                Outcome::Success
            };
            value.failures = 0;
            Ok(value)
        });

        assert_eq!(visited, ["blocked", "clean"]);
        assert!(matches!(report.accounts[0].outcome, Outcome::Blocked));
        assert!(matches!(report.accounts[1].outcome, Outcome::Success));
    }

    #[test]
    fn unavailable_failure_metrics_are_null() {
        let report = bit_mail::pull::failed_account_report(&account("failed"));
        let json = serde_json::to_value(report).unwrap();

        assert!(json["retries"].is_null());
        assert!(json["backlog_remaining"].is_null());
    }
}
