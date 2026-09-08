use std::{
    env,
    io::{IsTerminal, Read, Write},
    process::ExitCode,
    str::FromStr,
};

use bit_mail::{
    Result,
    cli::{
        AccountCommand, AttachmentCommand, CacheCommand, Cli, Command, ConfigCommand, IndexCommand,
        KnowledgeCommand, RawCommand, SelectionCommand,
    },
    credentials::{GoogleCredentialRevoker, KeyringStore},
    progress::{Event as ProgressEvent, Spinner},
    pull::{AccountReport, PullReport},
    repository::{AccountConfig, GitIgnorePolicy, RemoveOptions, Repository},
};
use clap::{CommandFactory, Parser};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", bit_mail::format::error(format!("error: {error}")));
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
                    eprintln!(
                        "{}",
                        bit_mail::format::error(format!(
                            "error: pull failed for {}: {error}",
                            account.alias
                        ))
                    );
                    bit_mail::pull::failed_account_report(account)
                }
            })
            .collect(),
    )
}

fn spinner_enabled(verbose: bool, json: bool) -> bool {
    !verbose && !json
}

fn tracing_level(verbose: bool) -> tracing::Level {
    if verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_target(false)
        .with_max_level(tracing_level(cli.verbose))
        .with_ansi(bit_mail::format::stderr_enabled())
        .with_writer(bit_mail::progress::stderr_writer)
        .try_init()?;
    match cli.command {
        None => {
            print_help()?;
        }
        Some(Command::Help { json }) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&bit_mail::harness::capabilities()?)?
                );
            } else {
                print_help()?;
            }
        }
        Some(Command::Init) => {
            let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
            let progress = |event| spinner.report(event);
            let repository = Repository::initialize_with_progress(
                &env::current_dir()?,
                GitIgnorePolicy::Prompt,
                &progress,
            )?;
            drop(spinner);
            println!(
                "{}",
                bit_mail::format::result(format!(
                    "Initialized bit-mail repository {} at {}",
                    repository.id(),
                    repository.root().display()
                ))
            );
        }
        Some(Command::Context { json: true }) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&bit_mail::harness::session_context(
                    &repository,
                    &account
                )?)?
            );
        }
        Some(Command::Context { json: false }) => unreachable!("--json is required by clap"),
        Some(Command::Doctor(args)) => {
            if args.all_accounts && cli.account.is_some() {
                return Err(
                    std::io::Error::other("--account cannot be used with --all-accounts").into(),
                );
            }
            let repository = match Repository::discover_current_for_diagnostics() {
                Ok(repository) => repository,
                Err(_) => {
                    let report = bit_mail::diagnostics::repository_failure();
                    if args.json {
                        println!("{}", serde_json::to_string_pretty(&report)?);
                    } else {
                        print!("{}", report.render());
                    }
                    return Err(std::io::Error::other("doctor found errors").into());
                }
            };
            let store = KeyringStore::new(repository.id());
            let spinner =
                Spinner::new(spinner_enabled(cli.verbose, args.json) && (args.full || args.online));
            let progress = |event| spinner.report(event);
            let report = bit_mail::diagnostics::run_with_progress(
                &repository,
                bit_mail::diagnostics::Options {
                    account: cli.account.as_deref(),
                    all_accounts: args.all_accounts,
                    full: args.full,
                    online: args.online,
                },
                &store,
                |account| {
                    use bit_mail::provider::MailProvider;
                    bit_mail::gmail::authorized_client(&repository, account, &store)?
                        .current_history_id()
                        .map(|_| ())
                },
                &progress,
            );
            drop(spinner);
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render());
            }
            if report.failed() {
                return Err(std::io::Error::other("doctor found errors").into());
            }
        }
        Some(Command::MigrateIntegrity) => {
            let repository = Repository::discover_current()?;
            let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
            let progress = |event| spinner.report(event);
            let migrated = repository.migrate_integrity_with_progress(&progress)?;
            drop(spinner);
            if migrated {
                println!(
                    "{}",
                    bit_mail::format::result("Migrated repository integrity to schema v2")
                );
            } else {
                println!(
                    "{}",
                    bit_mail::format::result("Repository integrity is already schema v2")
                );
            }
        }
        Some(Command::Connect { reauthorize }) => {
            let repository = Repository::discover_current()?;
            let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
            let progress = |event| spinner.report(event);
            bit_mail::connect::run_with_progress(&repository, reauthorize.as_deref(), &progress)?;
        }
        Some(Command::Config(args)) => {
            let repository = Repository::discover_current()?;
            match args.command {
                ConfigCommand::Show { json: true } => println!("{}", repository.config_json()?),
                ConfigCommand::Show { json: false } => print!("{}", repository.config_toml()?),
                ConfigCommand::Set { key, value } => {
                    repository.set_config(&key, &value)?;
                    println!("{}", bit_mail::format::result(format!("Updated {key}")));
                }
            }
        }
        Some(Command::Accounts) => {
            let accounts = Repository::discover_current()?.accounts()?;
            if accounts.is_empty() {
                if bit_mail::format::is_terminal() {
                    println!(
                        "{}",
                        bit_mail::format::cyan(
                            "No accounts configured.",
                            bit_mail::format::enabled()
                        )
                    );
                }
                return Ok(());
            }
            if bit_mail::format::is_terminal() {
                let lines = accounts
                    .iter()
                    .map(|account| {
                        format!("{} · {} · {}", account.alias, account.provider, account.id)
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    bit_mail::format::panel("Accounts", &lines, bit_mail::format::enabled())
                );
            } else {
                for account in accounts {
                    println!("{}\t{}\t{}", account.alias, account.id, account.provider);
                }
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
                    println!(
                        "{}",
                        bit_mail::format::result(format!(
                            "Renamed account to {} ({})",
                            account.alias, account.id
                        ))
                    );
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
                    println!(
                        "{}",
                        bit_mail::format::result(format!("Removed account {alias}"))
                    );
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
        Some(Command::Status(args)) => {
            let repository = Repository::discover_current()?;
            if args.all_accounts && cli.account.is_some() {
                return Err(
                    std::io::Error::other("--account cannot be used with --all-accounts").into(),
                );
            }
            let accounts = if args.all_accounts {
                repository.accounts()?
            } else {
                vec![resolve_account(&repository, cli.account.as_deref())?]
            };
            let report = bit_mail::status::report(&repository, accounts)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
                return Ok(());
            }
            let color = bit_mail::format::enabled();
            if report.accounts.is_empty() {
                if bit_mail::format::is_terminal() {
                    println!(
                        "{}",
                        bit_mail::format::cyan("No accounts configured.", color)
                    );
                }
                return Ok(());
            }
            let repository_path = report.repository.clone();
            for account in report.accounts {
                if !bit_mail::format::is_terminal() {
                    let backlog = account
                        .backlog_remaining
                        .map_or("unknown", |remaining| if remaining { "yes" } else { "no" });
                    let last_pull = account
                        .last_successful_pull_ms
                        .map_or_else(|| "-".into(), |value| value.to_string());
                    let last_push = account
                        .last_successful_push_ms
                        .map_or_else(|| "-".into(), |value| value.to_string());
                    println!(
                        "{}\tpending={}\tread={}\tdelete={}\tbacklog={}\tlast_pull_ms={}\tlast_push_ms={}",
                        account.alias,
                        account.pending,
                        account.read,
                        account.delete,
                        backlog,
                        last_pull,
                        last_push
                    );
                    continue;
                }
                let backlog = account.backlog_remaining.map_or("Unknown", |remaining| {
                    if remaining { "Remaining" } else { "Clear" }
                });
                let staged = account.read + account.delete;
                let next = next_hint(&account.alias, account.pending, staged);
                let context = vec![
                    format!("Repository      {}", repository_path),
                    format!("Account         {}", account.alias),
                    format!("Account ID      {}", account.account_id),
                    format!("Provider        {}", account.provider),
                    format!(
                        "Identity        {}",
                        account.provider_identity.as_deref().unwrap_or("Unknown")
                    ),
                    "Status          Local and offline".into(),
                ];
                let count = |value: usize| bit_mail::format::number(value, color);
                let work = vec![
                    format!("Pending         {}", count(account.pending)),
                    format!("Staged          {}", count(staged)),
                    format!("  Mark read     {}", count(account.read)),
                    format!("  Delete        {}", count(account.delete)),
                ];
                let sync = vec![
                    format!(
                        "Pull backlog    {}",
                        bit_mail::format::yellow(backlog, color)
                    ),
                    format!(
                        "Last pull       {}",
                        bit_mail::format::elapsed(account.last_successful_pull_ms)
                    ),
                    format!(
                        "Last push       {}",
                        bit_mail::format::elapsed(account.last_successful_push_ms)
                    ),
                ];
                let selections = if account.selections.is_empty() {
                    vec!["No selections".into()]
                } else {
                    let mut lines = account.selections.iter().map(|selection| format!(
                        "{} · {} messages (pending {} · read {} · delete {} · no work item {})",
                        selection.name, selection.message_count, selection.pending, selection.read, selection.delete, selection.no_work_item
                    )).collect::<Vec<_>>();
                    lines.push("Selections may overlap; counts are not additive.".into());
                    lines
                };
                println!(
                    "{}",
                    bit_mail::format::panel(
                        &format!("{} · status", account.alias),
                        &context,
                        color
                    )
                );
                println!("{}", bit_mail::format::panel("Work items", &work, color));
                println!(
                    "{}",
                    bit_mail::format::panel("Selections", &selections, color)
                );
                println!("{}", bit_mail::format::panel("Sync", &sync, color));
                println!("{}", bit_mail::format::panel("Next step", &[next], color));
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
            let spinner = Spinner::new(spinner_enabled(cli.verbose, args.json));
            let progress = |event| spinner.report(event);
            let report = pull_accounts(accounts, |account| {
                bit_mail::pull::pull_account_with_progress(
                    &repository,
                    account,
                    options,
                    || {
                        Ok(Box::new(bit_mail::gmail::authorized_client(
                            &repository,
                            account,
                            &store,
                        )?))
                    },
                    &progress,
                )
                .inspect_err(|_| progress(ProgressEvent::Suspend))
            });
            drop(spinner);
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_pull_result(&report);
            }
            if report.failed() {
                return Err(std::io::Error::other(
                    "pull completed with blocked or failed accounts",
                )
                .into());
            }
        }
        Some(Command::Push(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let scope = if let Some(message) = args.message {
                bit_mail::push::PushScope::Message(message)
            } else if let Some(selection) = args.selection.clone() {
                bit_mail::push::PushScope::Selection(selection)
            } else {
                bit_mail::push::PushScope::AllStaged
            };
            let store = KeyringStore::new(repository.id());
            let spinner = Spinner::new(spinner_enabled(cli.verbose, args.json));
            let progress = |event| spinner.report(event);
            let report = bit_mail::push::push_account_with_progress(
                &repository,
                &account,
                bit_mail::push::PushOptions {
                    scope,
                    dry_run: args.dry_run,
                },
                || {
                    Ok(Box::new(bit_mail::gmail::authorized_client(
                        &repository,
                        &account,
                        &store,
                    )?))
                },
                |stage, preview| review_push(stage, preview, args.yes),
                &progress,
            )?;
            drop(spinner);
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if args.dry_run {
                print_push_preview(&report, false);
            } else {
                print_push_result(&report);
            }
            if report.failed() {
                return Err(std::io::Error::other("push completed with failures").into());
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
            let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
            let progress = |event| spinner.report(event);
            let AttachmentCommand::Fetch {
                message_id,
                part_id,
            } = args.command;
            let path = bit_mail::pull::fetch_attachment_with_progress(
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
                &progress,
            )?;
            drop(spinner);
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
            let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
            let progress = |event| spinner.report(event);
            let RawCommand::Fetch { message_id } = args.command;
            let path = bit_mail::pull::fetch_raw_with_progress(
                &repository,
                &account,
                message_id,
                || {
                    Ok(Box::new(bit_mail::gmail::authorized_client(
                        &repository,
                        &account,
                        &store,
                    )?))
                },
                &progress,
            )?;
            drop(spinner);
            println!("{}", path.display());
        }
        Some(Command::WorkItems(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let output = bit_mail::triage::work_items(&repository, &account, args.state)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                if output.work_items.is_empty() {
                    if bit_mail::format::is_terminal() {
                        println!(
                            "{}",
                            bit_mail::format::cyan(
                                "No actionable work items.",
                                bit_mail::format::enabled()
                            )
                        );
                    }
                } else {
                    if bit_mail::format::is_terminal() {
                        let lines = output
                            .work_items
                            .iter()
                            .map(|item| {
                                format!(
                                    "{} · {} · {}",
                                    item.state,
                                    item.message_id,
                                    item.content_path.display()
                                )
                            })
                            .collect::<Vec<_>>();
                        println!(
                            "{}",
                            bit_mail::format::panel(
                                "Work items",
                                &lines,
                                bit_mail::format::enabled()
                            )
                        );
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
            }
        }
        Some(Command::Show {
            message_id,
            context,
            json,
        }) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let output = bit_mail::harness::show(&repository, &account, message_id, context)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                print!("{}", bit_mail::harness::render_show(&output)?);
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
            let message = if changed == 0 && bit_mail::format::is_terminal() {
                format!("No work items staged for {state}")
            } else {
                format!("Staged {changed} work item(s) {state}")
            };
            println!("{}", bit_mail::format::result(message));
        }
        Some(Command::Unstage(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let changed = match args.selection {
                Some(name) => bit_mail::triage::unstage_selection(&repository, &account, &name)?,
                None => bit_mail::triage::unstage(&repository, &account, &args.ids)?,
            };
            let message = if changed == 0 && bit_mail::format::is_terminal() {
                "No staged work items changed".into()
            } else {
                format!("Unstaged {changed} work item(s)")
            };
            println!("{}", bit_mail::format::result(message));
        }
        Some(Command::Selection(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let json = args.json;
            match args.command {
                SelectionCommand::List => {
                    let output = bit_mail::triage::list_selections(&repository, &account)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        if bit_mail::format::is_terminal() {
                            let lines = output
                                .selections
                                .iter()
                                .map(|selection| {
                                    format!(
                                        "{} · {} messages",
                                        selection.name, selection.message_count
                                    )
                                })
                                .collect::<Vec<_>>();
                            let lines = if lines.is_empty() {
                                vec!["No selections".into()]
                            } else {
                                lines
                            };
                            println!(
                                "{}",
                                bit_mail::format::panel(
                                    "Selections",
                                    &lines,
                                    bit_mail::format::enabled()
                                )
                            );
                        } else {
                            for selection in output.selections {
                                println!("{}\t{}", selection.name, selection.message_count);
                            }
                        }
                    }
                }
                SelectionCommand::Create { name } => {
                    let value = bit_mail::triage::create_selection(&repository, &account, &name)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&value)?);
                    } else {
                        println!(
                            "{}",
                            bit_mail::format::result(format!("Created selection {name}"))
                        );
                    }
                }
                SelectionCommand::Add { name, ids } => {
                    let value =
                        bit_mail::triage::add_selection(&repository, &account, &name, &ids)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&value)?);
                    } else {
                        println!(
                            "{}",
                            bit_mail::format::result(format!(
                                "Selection {} has {} item(s)",
                                value.name,
                                value.message_ids.len()
                            ))
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
                            "{}",
                            bit_mail::format::result(format!(
                                "Selection {} has {} item(s)",
                                value.name,
                                value.message_ids.len()
                            ))
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
                        println!(
                            "{}",
                            bit_mail::format::result(format!("Deleted selection {name}"))
                        );
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
                    println!(
                        "{}",
                        bit_mail::format::result(format!("Added Knowledge {}", item.id))
                    );
                }
                KnowledgeCommand::List { json } => {
                    let output = bit_mail::knowledge::list(&repository, account.as_ref())?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        if output.knowledge.is_empty() {
                            if bit_mail::format::is_terminal() {
                                println!(
                                    "{}",
                                    bit_mail::format::cyan(
                                        "No Knowledge items.",
                                        bit_mail::format::enabled()
                                    )
                                );
                            }
                        } else {
                            if bit_mail::format::is_terminal() {
                                let lines = output
                                    .knowledge
                                    .iter()
                                    .map(|item| {
                                        format!(
                                            "{} · {} · {}",
                                            item.id,
                                            item.scope,
                                            item.path.display()
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                println!(
                                    "{}",
                                    bit_mail::format::panel(
                                        "Knowledge",
                                        &lines,
                                        bit_mail::format::enabled()
                                    )
                                );
                            } else {
                                for item in output.knowledge {
                                    println!(
                                        "{}\t{}\t{}",
                                        item.id,
                                        item.scope,
                                        item.path.display()
                                    );
                                }
                            }
                        }
                    }
                }
                KnowledgeCommand::Show { id } => {
                    let item = bit_mail::knowledge::show(&repository, account.as_ref(), id)?;
                    print!("{}", item.content.expect("show includes content"));
                }
                KnowledgeCommand::Update { id, content } => {
                    bit_mail::knowledge::update(&repository, account.as_ref(), id, &content)?;
                    println!(
                        "{}",
                        bit_mail::format::result(format!("Updated Knowledge {id}"))
                    );
                }
                KnowledgeCommand::Remove { id } => {
                    bit_mail::knowledge::remove(&repository, account.as_ref(), id)?;
                    println!(
                        "{}",
                        bit_mail::format::result(format!("Removed Knowledge {id}"))
                    );
                }
            }
        }
        Some(Command::Repair { message_id }) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let store = KeyringStore::new(repository.id());
            let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
            let progress = |event| spinner.report(event);
            let report = bit_mail::recovery::repair_with_progress(
                &repository,
                &account,
                message_id,
                || {
                    Ok(Box::new(bit_mail::gmail::authorized_client(
                        &repository,
                        &account,
                        &store,
                    )?))
                },
                &progress,
            )?;
            drop(spinner);
            println!(
                "{}",
                bit_mail::format::result(format!(
                    "Repaired {} message(s); {} pending",
                    report.thread_messages, report.pending
                ))
            );
        }
        Some(Command::Gc(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
            let progress = |event| spinner.report(event);
            let report = bit_mail::recovery::gc_with_progress(
                &repository,
                &account,
                args.dry_run,
                &progress,
            )?;
            drop(spinner);
            let action = if args.dry_run {
                "Would remove"
            } else {
                "Removed"
            };
            println!(
                "{}",
                bit_mail::format::result(format!(
                    "{action} {} thread(s), {} message(s)",
                    report.threads,
                    report.messages.len()
                ))
            );
        }
        Some(Command::Cache(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            match args.command {
                CacheCommand::Rebuild => {
                    let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
                    let progress = |event| spinner.report(event);
                    bit_mail::recovery::cache_rebuild_with_progress(
                        &repository,
                        &account,
                        &progress,
                    )?;
                    drop(spinner);
                    println!(
                        "{}",
                        bit_mail::format::result(format!("Rebuilt cache for {}", account.alias))
                    );
                }
            }
        }
        Some(Command::Index(args)) => {
            let repository = Repository::discover_current()?;
            let account = resolve_account(&repository, cli.account.as_deref())?;
            match args.command {
                IndexCommand::Rebuild => {
                    let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
                    let progress = |event| spinner.report(event);
                    bit_mail::recovery::index_rebuild_with_progress(
                        &repository,
                        &account,
                        &progress,
                    )?;
                    drop(spinner);
                    println!(
                        "{}",
                        bit_mail::format::result(format!(
                            "Rebuilt structural index for {}",
                            account.alias
                        ))
                    );
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

fn next_hint(alias: &str, pending: usize, staged: usize) -> String {
    let command_prefix = format!("bit-mail --account {alias}");
    if staged > 0 {
        format!("Suggestion (local state): {command_prefix} push --dry-run")
    } else if pending > 0 {
        format!("Suggestion (local state): {command_prefix} work-items")
    } else {
        format!("Suggestion (local state): {command_prefix} pull")
    }
}

fn print_help() -> Result<()> {
    Cli::command().print_help()?;
    println!();
    Ok(())
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

fn review_push(
    stage: bit_mail::push::ReviewStage,
    report: &bit_mail::push::PushReport,
    yes: bool,
) -> Result<bool> {
    match stage {
        bit_mail::push::ReviewStage::Normal => print_push_preview(report, true),
        bit_mail::push::ReviewStage::ThreadedDelete => {
            eprintln!(
                "{}",
                bit_mail::format::yellow(
                    "Threaded delete risk:",
                    bit_mail::format::stderr_enabled()
                )
            );
            for item in report.items.iter().filter(|item| item.threaded_delete) {
                eprintln!(
                    "{}",
                    bit_mail::format::yellow(
                        format!("  delete {} from a multi-message thread", item.message_id),
                        bit_mail::format::stderr_enabled()
                    )
                );
            }
        }
    }
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err(std::io::Error::other(
            "push confirmation requires an interactive terminal; use --yes deliberately",
        )
        .into());
    }
    let prompt = match stage {
        bit_mail::push::ReviewStage::Normal => "Apply these staged actions? [y/N] ",
        bit_mail::push::ReviewStage::ThreadedDelete => {
            "Also confirm the threaded message deletes? [y/N] "
        }
    };
    eprint!(
        "{}",
        bit_mail::format::cyan(prompt, bit_mail::format::stderr_enabled())
    );
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn print_push_preview(report: &bit_mail::push::PushReport, stderr: bool) {
    let reads = report
        .items
        .iter()
        .filter(|item| item.action == bit_mail::push::PushAction::Read)
        .count();
    let deletes = report.items.len() - reads;
    let risks = report
        .items
        .iter()
        .filter(|item| item.threaded_delete)
        .count();
    let line = format!(
        "Push preview for {}: {reads} read, {deletes} delete, {risks} threaded-delete risk",
        report.account_alias
    );
    if stderr {
        eprintln!(
            "{}",
            bit_mail::format::cyan(&line, bit_mail::format::stderr_enabled())
        );
        for item in &report.items {
            eprintln!(
                "  {:?}\t{}{}",
                item.action,
                item.message_id,
                if item.threaded_delete {
                    if bit_mail::format::stderr_enabled() {
                        "\t\x1b[31mTHREADED DELETE\x1b[0m"
                    } else {
                        "\tTHREADED DELETE"
                    }
                } else {
                    ""
                }
            );
        }
    } else {
        if bit_mail::format::is_terminal() {
            println!(
                "{}",
                bit_mail::format::panel(
                    "Push preview",
                    &[
                        format!("Account         {}", report.account_alias),
                        format!("Mark read       {reads}"),
                        format!("Delete          {deletes}"),
                        format!("Threaded risk   {risks}")
                    ],
                    bit_mail::format::enabled()
                )
            );
        } else {
            println!("{line}");
        }
    }
}

fn print_push_result(report: &bit_mail::push::PushReport) {
    let line = format!(
        "Push {:?}: {} item(s), {} retries",
        report.outcome,
        report.items.len(),
        report.retries
    );
    if bit_mail::format::is_terminal() {
        if report.items.is_empty() {
            println!(
                "{}",
                bit_mail::format::cyan(
                    format!(
                        "No staged actions were applied for {}.",
                        report.account_alias
                    ),
                    bit_mail::format::enabled()
                )
            );
        } else {
            println!(
                "{}",
                bit_mail::format::panel(
                    "Push result",
                    &[
                        format!("Outcome         {:?}", report.outcome),
                        format!("Items           {}", report.items.len()),
                        format!("Retries         {}", report.retries)
                    ],
                    bit_mail::format::enabled()
                )
            );
        }
    } else {
        println!("{line}");
    }
    for item in &report.items {
        if item.outcome == bit_mail::push::ItemOutcome::Missing {
            eprintln!(
                "{}",
                bit_mail::format::yellow(
                    format!(
                        "warning: provider message {} is missing; resolved locally",
                        item.message_id
                    ),
                    bit_mail::format::stderr_enabled()
                )
            );
        }
    }
}

fn print_pull_result(report: &bit_mail::pull::PullReport) {
    if !bit_mail::format::is_terminal() {
        for account in &report.accounts {
            let retries = account
                .retries
                .map_or_else(|| "unknown".into(), |value| value.to_string());
            let backlog = account.backlog_remaining.map_or("unknown", |remaining| {
                if remaining { "remaining" } else { "clear" }
            });
            println!(
                "{}: {:?}; {} seeds, {} threads attempted, {} additional unread, {} new/{} removed work items, {} retries, {} failures, backlog {}",
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
        return;
    }
    if report.accounts.is_empty() {
        println!(
            "{}",
            bit_mail::format::cyan("No accounts to pull.", bit_mail::format::enabled())
        );
        return;
    }
    let color = bit_mail::format::enabled();
    for account in &report.accounts {
        let retries = account
            .retries
            .map_or_else(|| "Unknown".into(), |value| value.to_string());
        let backlog = account.backlog_remaining.map_or("Unknown", |remaining| {
            if remaining { "Remaining" } else { "Clear" }
        });
        let lines = vec![
            format!("Outcome         {:?}", account.outcome),
            format!("Threads attempted {}", account.threads),
            format!("New work        {}", account.new_work_items),
            format!("Removed work    {}", account.removed_work_items),
            format!("Retries         {retries}"),
            format!("Failures        {}", account.failures),
            format!(
                "Backlog         {}",
                bit_mail::format::yellow(backlog, color)
            ),
        ];
        println!(
            "{}",
            bit_mail::format::panel(&format!("{} · pull", account.alias), &lines, color)
        );
    }
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

    #[test]
    fn spinner_is_suppressed_for_verbose_and_json_output() {
        assert!(spinner_enabled(false, false));
        assert!(!spinner_enabled(true, false));
        assert!(!spinner_enabled(false, true));
    }

    #[test]
    fn default_tracing_keeps_operational_warnings_visible() {
        assert_eq!(tracing_level(false), tracing::Level::INFO);
        assert_eq!(tracing_level(true), tracing::Level::DEBUG);
    }

    #[test]
    fn status_hint_keeps_account_scope_and_declares_local_state() {
        assert_eq!(
            next_hint("other", 0, 0),
            "Suggestion (local state): bit-mail --account other pull"
        );
        assert_eq!(
            next_hint("other", 2, 0),
            "Suggestion (local state): bit-mail --account other work-items"
        );
        assert_eq!(
            next_hint("other", 2, 1),
            "Suggestion (local state): bit-mail --account other push --dry-run"
        );
    }
}
