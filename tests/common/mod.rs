use std::process::Command;

pub fn bit_mail() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bit-mail"))
}
