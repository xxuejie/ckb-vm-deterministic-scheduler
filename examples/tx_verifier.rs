use ckb_mock_tx_types::{MockTransaction, ReprMockTransaction};
use ckb_vm_deterministic_scheduler::dev_utils::verify_tx;
use clap::{command, Parser};
use serde_json::from_str as from_json_str;
use std::fs::read_to_string;
use std::io::{stdin, Read};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    tx_file: String,

    #[arg(short, long, default_value_t = 5_000_000)]
    cycles_per_iterate: u64,

    #[arg(short, long, default_value_t = 20_000_000)]
    cycles_per_suspend: u64,

    #[arg(short, long, default_value_t = 18446744073709551615)]
    max_cycles: u64,
}

fn main() {
    env_logger::init();

    let args = Args::parse();

    let mock_tx: MockTransaction = {
        let data = if args.tx_file == "-" {
            let mut buf = String::new();
            stdin().read_to_string(&mut buf).expect("read");
            buf
        } else {
            read_to_string(args.tx_file).expect("read")
        };

        let repr_mock_tx: ReprMockTransaction = from_json_str(&data).expect("json parsing");
        repr_mock_tx.into()
    };

    match verify_tx(
        &mock_tx,
        args.max_cycles,
        args.cycles_per_iterate,
        args.cycles_per_suspend,
    ) {
        Ok(cycles) => println!("Tx completes consuming {} cycles!", cycles),
        Err(e) => {
            println!("Tx error occurs: {}", e);
            std::process::exit(1);
        }
    }
}
