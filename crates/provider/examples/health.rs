//! Read-only connection check. Never prints identity, library or credentials.
use std::sync::{atomic::AtomicBool, Arc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = tide_provider::Provider::new()?;
    if !provider.is_connected() {
        return Err("No saved account session".into());
    }
    match provider.execute(
        tide_provider::Request::Library,
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(tide_provider::Response::Library { .. }) => println!("PASS: account library verified"),
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
