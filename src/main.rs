use acnl_treasuretown_mngr::{logging, run};

#[tokio::main]
async fn main() {
    logging::init();
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
