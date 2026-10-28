//! Small self-hostable shared relay: no audio runtime, accounts or operator key.
use tide_jam_lab::relay::{serve_shared, SharedRelay};
fn run() -> Result<(), &'static str> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("tide-jam-relay --endpoint https://jam.example.test --port 8765");
        println!(
            "Run behind the supplied same-machine TLS proxy; the backend binds only 127.0.0.1."
        );
        return Ok(());
    }
    let mut endpoint = None;
    let mut port = None;
    if args.len() != 4 {
        return Err("Provide --endpoint and --port; see --help");
    }
    for pair in args.as_chunks::<2>().0 {
        match pair[0].as_str() {
            "--endpoint" if endpoint.is_none() => endpoint = Some(pair[1].as_str()),
            "--port" if port.is_none() => {
                port = Some(pair[1].parse::<u16>().map_err(|_| "Invalid port")?)
            }
            _ => return Err("Unknown or duplicate relay option"),
        }
    }
    let config = SharedRelay::new(endpoint.ok_or("Missing endpoint")?)?;
    let port = port.filter(|port| *port != 0).ok_or("Invalid port")?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Cannot initialize relay")?
        .block_on(async {
            let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .map_err(|_| "Cannot bind loopback relay")?;
            println!("Shared Jam backend listening on 127.0.0.1:{port}; TLS proxy required.");
            let (_stop, receiver) = tokio::sync::watch::channel(false);
            serve_shared(listener, receiver, config)
                .await
                .map_err(|_| "Relay stopped")
        })
}
fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
