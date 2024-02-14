use ckb_chain_spec::consensus::ConsensusBuilder;
use ckb_mock_tx_types::{DummyResourceLoader, MockTransaction, ReprMockTransaction, Resource};
use ckb_script::{TransactionScriptsVerifier, TxVerifyEnv};
use ckb_types::core::{cell::resolve_transaction, HeaderView};
use ckb_vm::Error;
use ckb_vm_deterministic_scheduler::{
    types::{RunMode, TxData},
    Scheduler,
};
use clap::{command, Parser};
use serde_json::from_str as from_json_str;
use std::collections::HashSet;
use std::fs::read_to_string;
use std::io::{stdin, Read};
use std::sync::Arc;
use std::process::exit;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    tx_file: String,

    #[arg(short, long, default_value_t = 5_000_000)]
    cycles_per_round: u64,

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
    let resource = Resource::from_both(&mock_tx, DummyResourceLoader {}).expect("create resource");
    let resolved_tx = Arc::new(
        resolve_transaction(
            mock_tx.core_transaction(),
            &mut HashSet::new(),
            &resource,
            &resource,
        )
        .expect("resolving tx"),
    );

    let consensus = Arc::new(ConsensusBuilder::default().build());
    let tx_env = Arc::new(TxVerifyEnv::new_commit(
        &HeaderView::new_advanced_builder().build(),
    ));

    let groups: Vec<_> = {
        let verifier = TransactionScriptsVerifier::new(
            resolved_tx.clone(),
            resource.clone(),
            consensus.clone(),
            tx_env.clone(),
        );
        verifier
            .groups_with_type()
            .map(|(a, b, c)| (a, b.clone(), c.clone()))
            .collect()
    };

    for (t, hash, group) in groups {
        log::debug!("Running {} of hash {:#x}", t, hash);

        let verifier = TransactionScriptsVerifier::new(
            resolved_tx.clone(),
            resource.clone(),
            consensus.clone(),
            tx_env.clone(),
        );
        let program = verifier
            .extract_script(&group.script)
            .expect("extracting program");

        let tx_data = TxData {
            rtx: resolved_tx.clone(),
            data_loader: resource.clone(),
            program,
            script_group: Arc::new(group),
        };

        let mut scheduler = Scheduler::new(tx_data, verifier);

        loop {
            if scheduler.consumed_cycles() > args.max_cycles {
                log::error!("{} of hash {} runs out of max cycles!", t, hash);
                exit(1);
            }

            log::debug!(
                "Run {} of hash {} with {} limit cycles",
                t,
                hash,
                args.cycles_per_round
            );
            match scheduler.run(RunMode::LimitCycles(args.cycles_per_round)) {
                Ok((exit_code, total_cycles)) => {
                    if total_cycles > args.max_cycles {
                        log::error!("{} of hash {} runs out of max cycles!", t, hash);
                        return;
                    }
                    log::info!(
                        "{} of hash {} terminates, exit code: {}, consumed cycles: {}",
                        t,
                        hash,
                        exit_code,
                        total_cycles
                    );
                    break;
                }
                Err(Error::CyclesExceeded) => (),
                Err(e) => {
                    log::error!("{} of hash {} encounters error: {:?}", t, hash, e);
                    exit(1);
                }
            }
        }
    }
}
