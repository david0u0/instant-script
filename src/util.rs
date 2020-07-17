use crate::error::Result;
use crate::script::Script;
use std::process::Command;

pub fn run(script: &Script, args: Vec<String>) -> Result<()> {
    let mut cmd = Command::new("sh");
    let mut full_args = vec![script.path.clone()];
    full_args.extend(args.into_iter().map(|s| s.into()));
    cmd.args(full_args).spawn()?.wait()?;
    Ok(())
}
