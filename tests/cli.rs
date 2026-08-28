mod common;

use std::fs;

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
    let knowledge_dir = directory
        .path()
        .join("knowledge/accounts")
        .join(personal.id.to_string());
    fs::create_dir(&knowledge_dir).expect("account Knowledge directory");
    fs::write(knowledge_dir.join("preference.md"), "keep me").expect("account Knowledge");

    let remove = common::bit_mail()
        .current_dir(directory.path())
        .args(["account", "remove", "private_mail", "--discard-local-data"])
        .output()
        .expect("remove must start");
    assert!(remove.status.success());
    assert!(String::from_utf8_lossy(&remove.stderr).contains("preserved account Knowledge"));
    assert!(repository.account_by_alias("private_mail").is_err());
    assert_eq!(
        fs::read_to_string(knowledge_dir.join("preference.md")).expect("preserved Knowledge"),
        "keep me"
    );
}
