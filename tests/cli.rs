mod common;

use std::{fs, io::Write, process::Stdio};

use bit_mail::repository::{NewAccount, Repository};

#[test]
fn help_is_available_without_provider_setup() {
    let output = common::bit_mail()
        .arg("--help")
        .output()
        .expect("bit-mail must start");

    assert!(
        output.status.success(),
        "help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: bit-mail"));

    let machine = common::bit_mail()
        .args(["help", "--json"])
        .output()
        .expect("machine help must start");
    assert!(machine.status.success());
    let value: serde_json::Value = serde_json::from_slice(&machine.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(value["commands"].as_array().unwrap().iter().any(|command| {
        command["path"] == serde_json::json!(["push"]) && command["may_mutate_provider"] == true
    }));
}

#[test]
fn init_and_config_work_without_provider_setup() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let init = common::bit_mail()
        .current_dir(directory.path())
        .arg("init")
        .output()
        .expect("bit-mail init must start");
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let set = common::bit_mail()
        .current_dir(directory.path())
        .args(["config", "set", "pull.default-limit", "1000"])
        .output()
        .expect("config set must start");
    assert!(set.status.success());

    let show = common::bit_mail()
        .current_dir(directory.path())
        .args(["config", "show", "--json"])
        .output()
        .expect("config show must start");
    assert!(show.status.success());
    assert!(String::from_utf8_lossy(&show.stdout).contains("\"default_limit\": 1000"));
}

#[test]
fn account_commands_use_uuid_owned_state_without_provider_setup() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let init = common::bit_mail()
        .current_dir(directory.path())
        .arg("init")
        .output()
        .expect("bit-mail init must start");
    assert!(init.status.success());

    let repository = Repository::open(directory.path().to_path_buf()).expect("repository");
    let personal = repository
        .create_account(NewAccount {
            alias: "personal",
            provider: "gmail",
            provider_identity: Some("person@example.com"),
            credential_profile: None,
        })
        .expect("personal account");
    let work = repository
        .create_account(NewAccount {
            alias: "work",
            provider: "gmail",
            provider_identity: Some("work@example.com"),
            credential_profile: None,
        })
        .expect("work account");

    let context = common::bit_mail()
        .current_dir(directory.path())
        .args(["--account", "personal", "context", "--json"])
        .output()
        .expect("context must start");
    assert!(context.status.success());
    let context: serde_json::Value = serde_json::from_slice(&context.stdout).unwrap();
    assert_eq!(context["account"]["id"], personal.id.to_string());
    assert_eq!(context["pull_blocked"], false);
    assert!(context.get("provider_identity").is_none());

    let accounts = common::bit_mail()
        .current_dir(directory.path())
        .arg("accounts")
        .output()
        .expect("accounts must start");
    assert!(accounts.status.success());
    assert!(String::from_utf8_lossy(&accounts.stdout).contains("personal"));

    let explicit_path = common::bit_mail()
        .current_dir(directory.path())
        .args(["--account", "personal", "path"])
        .output()
        .expect("explicit path must start");
    assert!(explicit_path.status.success());
    assert_eq!(
        String::from_utf8_lossy(&explicit_path.stdout).trim(),
        repository.data_dir(personal.id).display().to_string()
    );

    let inferred_path = common::bit_mail()
        .current_dir(repository.data_dir(personal.id))
        .arg("path")
        .output()
        .expect("inferred path must start");
    assert!(inferred_path.status.success());
    assert_eq!(inferred_path.stdout, explicit_path.stdout);

    let all_paths = common::bit_mail()
        .current_dir(directory.path())
        .args(["path", "--all-accounts"])
        .output()
        .expect("all-account paths must start");
    assert!(all_paths.status.success());
    assert_eq!(
        String::from_utf8_lossy(&all_paths.stdout),
        format!(
            "personal\t{}\nwork\t{}\n",
            repository.data_dir(personal.id).display(),
            repository.data_dir(work.id).display()
        )
    );

    let conflicting_scope = common::bit_mail()
        .current_dir(directory.path())
        .args(["--account", "personal", "path", "--all-accounts"])
        .output()
        .expect("conflicting path scope must start");
    assert!(!conflicting_scope.status.success());
    assert!(String::from_utf8_lossy(&conflicting_scope.stderr).contains("cannot be used with"));

    let prohibited_all = common::bit_mail()
        .current_dir(directory.path())
        .args(["account", "remove", "personal", "--all-accounts"])
        .output()
        .expect("invalid account removal must start");
    assert!(!prohibited_all.status.success());
    assert!(String::from_utf8_lossy(&prohibited_all.stderr).contains("unexpected argument"));

    let rename = common::bit_mail()
        .current_dir(directory.path())
        .args(["account", "rename", "personal", "private_mail"])
        .output()
        .expect("rename must start");
    assert!(rename.status.success());

    fs::write(repository.data_dir(personal.id).join("mail"), "private").expect("local mail");
    let renamed = repository
        .account_by_alias("private_mail")
        .expect("renamed account");
    let knowledge = bit_mail::knowledge::add(&repository, Some(&renamed), "keep me")
        .expect("account Knowledge");
    let knowledge_before = fs::read(&knowledge.path).expect("serialized account Knowledge");

    let remove = common::bit_mail()
        .current_dir(directory.path())
        .args(["account", "remove", "private_mail", "--discard-local-data"])
        .output()
        .expect("remove must start");
    assert!(remove.status.success());
    assert!(String::from_utf8_lossy(&remove.stderr).contains("preserved account Knowledge"));
    assert!(repository.account_by_alias("private_mail").is_err());
    assert_eq!(
        fs::read(&knowledge.path).expect("preserved Knowledge"),
        knowledge_before
    );
    assert!(
        bit_mail::integrity::validate_full(&repository)
            .expect("validate preserved Knowledge")
            .mismatches
            .is_empty()
    );
    fs::write(&knowledge.path, "tampered orphan Knowledge").expect("tamper preserved Knowledge");
    assert!(
        bit_mail::integrity::validate_full(&repository)
            .expect("detect preserved Knowledge tamper")
            .mismatches
            .iter()
            .any(|item| item.path.ends_with(&format!("{}.md", knowledge.id)))
    );
}

#[test]
fn invalid_stdin_batch_changes_no_offline_work_items() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let repository = Repository::initialize(
        directory.path(),
        bit_mail::repository::GitIgnorePolicy::Never,
    )
    .expect("repository");
    let account = repository
        .create_account(NewAccount {
            alias: "personal",
            provider: "gmail",
            provider_identity: Some("person@example.com"),
            credential_profile: None,
        })
        .expect("account");
    let id = uuid::Uuid::now_v7();
    let work_items = directory
        .path()
        .join(".bit-mail/accounts")
        .join(account.id.to_string())
        .join("work-items");
    fs::create_dir_all(&work_items).unwrap();
    let path = work_items.join(format!("{id}.json"));
    fs::write(
        &path,
        format!("{{\"schema_version\":1,\"message_id\":\"{id}\",\"state\":\"pending\"}}"),
    )
    .unwrap();

    let mut child = common::bit_mail()
        .current_dir(directory.path())
        .args(["stage", "--stdin", "read"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("stage must start without credentials");
    writeln!(child.stdin.take().unwrap(), "{id}\nnot-a-uuid").unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(!output.status.success());
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("\"state\":\"pending\"")
    );
}
