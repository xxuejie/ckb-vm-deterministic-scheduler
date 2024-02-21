use crate::dev_utils::{build_mock_tx, generate_data_graph, verify_tx};
use ckb_types::core::Cycle;
use proptest::prelude::*;

const MAX_CYCLES: Cycle = 300_000_000;
const CYCLES_PER_ITERATE: Cycle = 10_000_000;
const CYCLES_PER_SUSPEND: Cycle = 50_000_000;

#[test]
fn test_program_exists() {
    let program_path = match std::env::var("TEST_BIN") {
        Ok(path) => path,
        Err(_) => "./test_bin".to_string(),
    };
    let _ = std::fs::read(program_path).expect("read");
}

#[test]
fn test_single_dag() {
    let seed = 0;
    let spawns = 62;
    let writes = 168;

    let data = generate_data_graph(seed, spawns, writes, 3).expect("generate dag");
    let program_path = match std::env::var("TEST_BIN") {
        Ok(path) => path,
        Err(_) => "./test_bin".to_string(),
    };
    let program = std::fs::read(program_path).expect("read").into();

    let mock_tx = build_mock_tx(seed.wrapping_add(10), program, data);

    let result = verify_tx(&mock_tx, MAX_CYCLES, CYCLES_PER_ITERATE, CYCLES_PER_SUSPEND);
    assert!(result.unwrap() <= MAX_CYCLES);
}

proptest! {
    #[test]
    fn test_random_dag(
        seed: u64,
        spawns in 5u32..101u32,
        writes in 3u32..201u32,
    ) {
        let data = generate_data_graph(seed, spawns, writes, 3).expect("generate dag");
        let program_path = match std::env::var("TEST_BIN") {
            Ok(path) => path,
            Err(_) => "./test_bin".to_string(),
        };
        let program = std::fs::read(program_path).expect("read").into();

        let mock_tx = build_mock_tx(seed.wrapping_add(10), program, data);

        let result = verify_tx(&mock_tx, MAX_CYCLES, CYCLES_PER_ITERATE, CYCLES_PER_SUSPEND);
        assert!(result.unwrap() <= MAX_CYCLES);
    }
}
