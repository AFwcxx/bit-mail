mod common;

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
