use rml_auth::{Account, MicrosoftAuth, PollOutcome};

fn print_accounts() {
    let accounts = rml_auth::load_accounts();
    if accounts.is_empty() {
        println!("(no accounts)");
        return;
    }
    for a in accounts {
        println!("{:?}\t{}\t{}", a.kind, a.username, a.uuid);
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("offline") => {
            let name = args.get(1).map(String::as_str).unwrap_or("");
            if name.is_empty() {
                eprintln!("usage: rml-app offline <username>");
                std::process::exit(2);
            }
            let acc = Account::offline(name);
            match rml_auth::upsert_account(&acc) {
                Ok(_) => println!("added offline account '{}' ({})", acc.username, acc.uuid),
                Err(e) => eprintln!("failed: {e}"),
            }
        }
        Some("accounts") | None => print_accounts(),
        Some("login") => {
            let auth = MicrosoftAuth::new();
            let device = match auth.request_device_code().await {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("device code request failed: {e}");
                    std::process::exit(1);
                }
            };
            println!(
                "{}",
                device.message.clone().unwrap_or_else(|| format!(
                    "Go to {} and enter code {}",
                    device.verification_uri, device.user_code
                ))
            );
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(device.interval.max(5))).await;
                match auth.poll(&device.device_code).await {
                    Ok(PollOutcome::Success(a)) => {
                        let _ = rml_auth::upsert_account(&a);
                        println!("logged in as {}", a.username);
                        break;
                    }
                    Ok(PollOutcome::Pending | PollOutcome::SlowDown) => {}
                    Err(e) => {
                        eprintln!("login failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            eprintln!("commands: accounts | offline <name> | login");
            std::process::exit(2);
        }
    }
}
