#[cfg(not(windows))]
fn main() {
    eprintln!("windows-sandbox-probe is Windows-only");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    use std::{
        env, fs,
        io::Write,
        net::{SocketAddr, TcpStream},
        process::Command,
        time::Duration,
    };
    let args: Vec<_> = env::args().collect();
    let result: Result<(), String> = (|| match args.get(1).map(String::as_str) {
        Some("read-file") => fs::read(args.get(2).ok_or_else(|| "path".to_string())?)
            .map(|b| println!("{}", b.len()))
            .map_err(|e| e.to_string()),
        Some("write-file") => fs::write(args.get(2).ok_or_else(|| "path".to_string())?, b"probe")
            .map_err(|e| e.to_string()),
        Some("connect") => args
            .get(2)
            .ok_or_else(|| "address".to_string())
            .and_then(|s| s.parse::<SocketAddr>().map_err(|e| e.to_string()))
            .and_then(|a| {
                TcpStream::connect_timeout(&a, Duration::from_secs(1))
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }),
        Some("spawn-child") => Command::new(args.get(2).ok_or_else(|| "executable".to_string())?)
            .status()
            .map(|s| println!("{s}"))
            .map_err(|e| e.to_string()),
        Some("allocate-memory") => {
            let count = args
                .get(2)
                .ok_or_else(|| "bytes".to_string())?
                .parse::<usize>()
                .map_err(|e| e.to_string())?;
            let mut bytes = Vec::with_capacity(count);
            bytes.resize(count, 1);
            std::io::stdout()
                .write_all(&bytes[..1])
                .map_err(|e| e.to_string())
        }
        Some("print-environment") => {
            for (k, v) in env::vars() {
                println!("{k}={v}");
            }
            Ok(())
        }
        _ => Err(
            "usage: read-file|write-file|connect|spawn-child|allocate-memory|print-environment"
                .into(),
        ),
    })();
    if let Err(error) = result {
        eprintln!("DENIED_OR_FAILED: {error}");
        std::process::exit(1);
    }
}
