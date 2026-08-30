use std::{env, io::Read, process::ExitCode, str::FromStr};

use bit_mail::{
    Result,
    cli::{
        AccountCommand, AttachmentCommand, CacheCommand, Cli, Command, ConfigCommand,
        KnowledgeCommand, RawCommand, SelectionCommand,
    },
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
        Some(Command::MigrateIntegrity) => {
            let repository = Repository::discover_current()?;
            if repository.migrate_integrity()? {
                println!("Migrated repository integrity to schema v2");
            } else {
                println!("Repository integrity is already schema v2");
            }
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
        Some(Command::WorkItems(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let output = bit_mail::triage::work_items(&repository, &account, args.state)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                for item in output.work_items {
                    println!(
                        "{}\t{}\t{}",
                        item.state,
                        item.message_id,
                        item.content_path.display()
                    );
                }
            }
        }
        Some(Command::Stage(mut args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let action = args.values.pop().expect("clap requires an action");
            let state =
                bit_mail::triage::WorkState::from_str(&action).map_err(std::io::Error::other)?;
            let changed = if let Some(selection) = args.selection {
                if !args.values.is_empty() {
                    return Err(std::io::Error::other(
                        "message IDs cannot be used with --selection",
                    )
                    .into());
                }
                bit_mail::triage::stage_selection(&repository, &account, &selection, state)?
            } else {
                let ids = if args.stdin {
                    if !args.values.is_empty() {
                        return Err(std::io::Error::other(
                            "message IDs cannot be used with --stdin",
                        )
                        .into());
                    }
                    stdin_ids()?
                } else {
                    args.values
                        .iter()
                        .map(|value| value.parse())
                        .collect::<std::result::Result<Vec<_>, _>>()?
                };
                bit_mail::triage::stage(&repository, &account, &ids, state)?
            };
            println!("Staged {changed} work item(s) {state}");
        }
        Some(Command::Unstage(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let changed = match args.selection {
                Some(name) => bit_mail::triage::unstage_selection(&repository, &account, &name)?,
                None => bit_mail::triage::unstage(&repository, &account, &args.ids)?,
            };
            println!("Unstaged {changed} work item(s)");
        }
        Some(Command::Selection(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let json = args.json;
            match args.command {
                SelectionCommand::Create { name } => {
                    let value = bit_mail::triage::create_selection(&repository, &account, &name)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&value)?);
                    } else {
                        println!("Created selection {name}");
                    }
                }
                SelectionCommand::Add { name, ids } => {
                    let value =
                        bit_mail::triage::add_selection(&repository, &account, &name, &ids)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&value)?);
                    } else {
                        println!(
                            "Selection {} has {} item(s)",
                            value.name,
                            value.message_ids.len()
                        );
                    }
                }
                SelectionCommand::Remove { name, ids } => {
                    let value = bit_mail::triage::remove_selection_members(
                        &repository,
                        &account,
                        &name,
                        &ids,
                    )?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&value)?);
                    } else {
                        println!(
                            "Selection {} has {} item(s)",
                            value.name,
                            value.message_ids.len()
                        );
                    }
                }
                SelectionCommand::Show { name } => {
                    let value = bit_mail::triage::show_selection(&repository, &account, &name)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&value)?);
                    } else {
                        for id in value.message_ids {
                            println!("{id}");
                        }
                    }
                }
                SelectionCommand::Delete { name } => {
                    let value = bit_mail::triage::delete_selection(&repository, &account, &name)?;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "schema_version": value.schema_version,
                                "account_id": value.account_id,
                                "name": value.name,
                                "deleted": true
                            }))?
                        );
                    } else {
                        println!("Deleted selection {name}");
                    }
                }
            }
        }
        Some(Command::Knowledge(args)) => {
            let repository = Repository::discover_current()?;
            let account = cli
                .account
                .as_deref()
                .map(|alias| repository.account_by_alias(alias))
                .transpose()?;
            match args.command {
                KnowledgeCommand::Add { content } => {
                    let item = bit_mail::knowledge::add(&repository, account.as_ref(), &content)?;
                    println!("Added Knowledge {}", item.id);
                }
                KnowledgeCommand::List { json } => {
                    let output = bit_mail::knowledge::list(&repository, account.as_ref())?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        for item in output.knowledge {
                            println!("{}\t{}\t{}", item.id, item.scope, item.path.display());
                        }
                    }
                }
                KnowledgeCommand::Show { id } => {
                    let item = bit_mail::knowledge::show(&repository, account.as_ref(), id)?;
                    print!("{}", item.content.expect("show includes content"));
                }
                KnowledgeCommand::Update { id, content } => {
                    bit_mail::knowledge::update(&repository, account.as_ref(), id, &content)?;
                    println!("Updated Knowledge {id}");
                }
                KnowledgeCommand::Remove { id } => {
                    bit_mail::knowledge::remove(&repository, account.as_ref(), id)?;
                    println!("Removed Knowledge {id}");
                }
            }
        }
        Some(Command::Repair { message_id }) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let store = KeyringStore::new(repository.id());
            let report = bit_mail::recovery::repair(&repository, &account, message_id, || {
                Ok(Box::new(bit_mail::gmail::authorized_client(
                    &repository,
                    &account,
                    &store,
                )?))
            })?;
            println!(
                "Repaired {} message(s); {} pending",
                report.thread_messages, report.pending
            );
        }
        Some(Command::Gc(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let report = bit_mail::recovery::gc(&repository, &account, args.dry_run)?;
            let action = if args.dry_run {
                "Would remove"
            } else {
                "Removed"
            };
            println!(
                "{action} {} thread(s), {} message(s)",
                report.threads,
                report.messages.len()
            );
        }
        Some(Command::Cache(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            match args.command {
                CacheCommand::Rebuild => {
                    bit_mail::recovery::cache_rebuild(&repository, &account)?;
                    println!("Rebuilt cache for {}", account.alias);
                }
            }
        }
    }

    Ok(())
}

fn resolve_account(repository: &Repository, explicit: Option<&str>) -> Result<AccountConfig> {
    repository.resolve_account(
        explicit,
        &env::current_dir()?,
        env::var("BIT_MAIL_ACCOUNT").ok().as_deref(),
    )
}

fn stdin_ids() -> Result<Vec<uuid::Uuid>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let mut ids = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            return Err(std::io::Error::other(format!("stdin line {} is empty", index + 1)).into());
        }
        ids.push(line.parse()?);
    }
    if ids.is_empty() {
        return Err(std::io::Error::other("stdin contained no message IDs").into());
    }
    Ok(ids)
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
