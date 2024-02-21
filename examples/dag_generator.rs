use ckb_mock_tx_types::ReprMockTransaction;
use ckb_types::prelude::*;
use ckb_vm_deterministic_scheduler::dev_utils::{
    build_mock_tx, dag, generate_data_graph, verify_tx,
};
use clap::{command, Parser};
use molecule::prelude::*;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    output_tx_file: Option<String>,

    #[arg(long)]
    seed: Option<u64>,

    #[arg(long, default_value_t = 5)]
    spawns: u32,

    #[arg(long, default_value_t = 5)]
    writes: u32,

    #[arg(long, default_value_t = 2)]
    converging_threshold: u32,

    #[arg(long, default_value = "./test_bin")]
    test_contract_path: String,

    #[arg(short, long, default_value_t = 100_000_000)]
    max_cycles: u64,
}

fn main() {
    env_logger::init();

    let args = Args::parse();

    let seed = match args.seed {
        Some(val) => val,
        None => std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64,
    };
    println!("Seed: {}", seed);

    let data = generate_data_graph(seed, args.spawns, args.writes, args.converging_threshold)
        .expect("generating dag");

    // Visualize data
    for pipe in data.pipes().into_iter() {
        let vm = vm_index_to_u64(&pipe.vm());
        let read_pipe = pipe_index_to_u64(&pipe.read_pipe());
        let write_pipe = pipe_index_to_u64(&pipe.write_pipe());

        println!(
            "VM {} creates read pipe {}, write pipe {}",
            vm, read_pipe, write_pipe
        );
    }
    for spawn in data.spawns().into_iter() {
        let from = vm_index_to_u64(&spawn.from());
        let child = vm_index_to_u64(&spawn.child());
        let pipes: Vec<u64> = spawn
            .pipes()
            .into_iter()
            .map(|p| pipe_index_to_u64(&p))
            .collect();

        println!("VM {} spawns VM {}, passed pipes: {:?}", from, child, pipes);
    }
    for write in data.writes().into_iter() {
        let from = vm_index_to_u64(&write.from());
        let from_pipe = pipe_index_to_u64(&write.from_pipe());
        let to = vm_index_to_u64(&write.to());
        let to_pipe = pipe_index_to_u64(&write.to_pipe());
        let data_length = write.data().raw_data().len();

        println!(
            "VM {} writes {} bytes to pipe {}, read by VM {} from pipe {}",
            from, data_length, from_pipe, to, to_pipe
        );
    }

    // Build the transaction
    let program = std::fs::read(args.test_contract_path).expect("read").into();
    let mock_tx = build_mock_tx(seed.wrapping_add(10), program, data);

    // Dump the transaction if requested
    if let Some(output_tx_file) = args.output_tx_file {
        let repr_tx: ReprMockTransaction = mock_tx.clone().into();
        let json = serde_json::to_vec_pretty(&repr_tx).expect("to json");
        std::fs::write(output_tx_file, json).expect("write");
    }

    // Validate the actual transaction
    match verify_tx(&mock_tx, args.max_cycles, args.max_cycles, args.max_cycles) {
        Ok(cycles) => println!("Tx completes consuming {} cycles!", cycles),
        Err(e) => {
            println!("Tx error occurs: {:?}", e);
            std::process::exit(1);
        }
    }
}

fn vm_index_to_u64(i: &dag::VmIndex) -> u64 {
    let mut data = [0u8; 8];
    data.copy_from_slice(i.as_slice());
    u64::from_le_bytes(data)
}

fn pipe_index_to_u64(i: &dag::PipeIndex) -> u64 {
    let mut data = [0u8; 8];
    data.copy_from_slice(i.as_slice());
    u64::from_le_bytes(data)
}
