//! Read-only connection check. Never prints identity, library or credentials.
use std::sync::{atomic::AtomicBool, Arc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let expected = match args.as_slice() {
        [] => None,
        [flag, account] if flag == "--expected-account" => Some(account),
        _ => return Err("Use --expected-account followed by the public profile name".into()),
    };
    let provider = tide_provider::Provider::new()?;
    if !provider.is_connected() {
        return Err("No saved account session".into());
    }
    match provider.execute(
        tide_provider::Request::Library,
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(tide_provider::Response::Library { account, .. }) => {
            if expected.is_some_and(|expected| !account.eq_ignore_ascii_case(expected)) {
                return Err("Connected profile does not match the expected Music profile".into());
            }
            println!("PASS: account library verified");
            if expected.is_some() {
                println!("PASS: selected Music profile matches the expected profile");
            }
        }
        Ok(tide_provider::Response::Error { code }) if code == "session" => {
            return Err("Account authentication failed".into())
        }
        Ok(tide_provider::Response::Error { .. }) => {
            return Err("Account service request failed".into());
        }
        Ok(_) => return Err("Unexpected account response".into()),
        Err(_) => return Err("Account helper or request failed".into()),
    }
    Ok(())
}
