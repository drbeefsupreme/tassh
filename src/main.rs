mod cli;
mod clipboard;
mod display;
mod protocol;
mod transport;

use clap::Parser;
use cli::Commands;

#[tokio::main]
async fn main() {
    let cli = cli::Cli::parse();

    match cli.command {
        Commands::Local(args) => {
            println!("cssh local: remote={} port={}", args.remote, args.port);
        }
        Commands::Remote(args) => {
            println!("cssh remote: port={}", args.port);
        }
        Commands::Status => {
            println!("status: not yet implemented");
        }
    }
}
