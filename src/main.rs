use std::{env, process::ExitCode};

use bit_mail::{
    Result,
    cli::{AccountCommand, Cli, Command, ConfigCommand},
    credentials::{GoogleCredentialRevoker, KeyringStore},
    repository::{GitIgnorePolicy, RemoveOptions, Repository},
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
    }

    Ok(())
}
