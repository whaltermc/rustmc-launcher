#[tokio::main]
async fn main() {
    println!("RustMC Launcher (CLI)");
    if let Some(a) = rml_auth::load_auth() {
        println!("Cached login: {}", a.username);
    } else {
        println!("Not logged in");
    }
}
