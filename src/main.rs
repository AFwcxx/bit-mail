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
        .with_writer(bit_mail::progress::stderr_writer)
        .try_init()?;
    match cli.command {
        None => {
            Cli::command().print_help()?;
            println!();
        }
        Some(Command::Help { json }) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&bit_mail::harness::capabilities()?)?
                );
            } else {
                Cli::command().print_help()?;
                println!();
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
                "Initialized bit-mail repository {} at {}",
                repository.id(),
                repository.root().display()
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
                println!("Migrated repository integrity to schema v2");
            } else {
                println!("Repository integrity is already schema v2");
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
            for account in bit_mail::status::collect(&repository, accounts)? {
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
                SelectionCommand::List => {
                    let output = bit_mail::triage::list_selections(&repository, &account)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        for selection in output.selections {
                            println!("{}\t{}", selection.name, selection.message_count);
                        }
                    }
                }
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
                "Repaired {} message(s); {} pending",
                report.thread_messages, report.pending
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
                    let spinner = Spinner::new(spinner_enabled(cli.verbose, false));
                    let progress = |event| spinner.report(event);
                    bit_mail::recovery::cache_rebuild_with_progress(
                        &repository,
                        &account,
                        &progress,
                    )?;
                    drop(spinner);
                    println!("Rebuilt cache for {}", account.alias);
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
                    println!("Rebuilt structural index for {}", account.alias);
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

fn review_push(
    stage: bit_mail::push::ReviewStage,
    report: &bit_mail::push::PushReport,
    yes: bool,
) -> Result<bool> {
    match stage {
        bit_mail::push::ReviewStage::Normal => print_push_preview(report, true),
        bit_mail::push::ReviewStage::ThreadedDelete => {
            eprintln!("Threaded delete risk:");
            for item in report.items.iter().filter(|item| item.threaded_delete) {
                eprintln!("  delete {} from a multi-message thread", item.message_id);
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
    eprint!("{prompt}");
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
        eprintln!("{line}");
        for item in &report.items {
            eprintln!(
                "  {:?}\t{}{}",
                item.action,
                item.message_id,
                if item.threaded_delete {
                    "\tTHREADED DELETE"
                } else {
                    ""
                }
            );
        }
    } else {
        println!("{line}");
    }
}

fn print_push_result(report: &bit_mail::push::PushReport) {
    println!(
        "Push {:?}: {} item(s), {} retries",
        report.outcome,
        report.items.len(),
        report.retries
    );
    for item in &report.items {
        if item.outcome == bit_mail::push::ItemOutcome::Missing {
            eprintln!(
                "warning: provider message {} is missing; resolved locally",
                item.message_id
            );
        }
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
}
