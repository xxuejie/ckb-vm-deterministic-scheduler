//! This module contains utils that are handy for tests & examples.
//! The scheduler itself does not require code in here.

use crate::{
    types::{RunMode, TxData},
    Scheduler,
};
use ckb_chain_spec::consensus::ConsensusBuilder;
use ckb_mock_tx_types::{
    DummyResourceLoader, MockCellDep, MockInfo, MockInput, MockTransaction, Resource,
};
use ckb_script::{TransactionScriptsVerifier, TxVerifyEnv};
use ckb_types::{
    bytes::Bytes,
    core::{
        cell::resolve_transaction, hardfork::HardForks, Cycle, DepType, HeaderView, ScriptHashType,
        TransactionBuilder,
    },
    packed::{CellDep, CellInput, CellOutput, OutPoint, Script},
    prelude::*,
};
use ckb_vm::Error;
use daggy::{Dag, Walker};
use molecule::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

pub mod dag;

/// Given a full CKB transaction, this method runs all scripts in it
/// and either return full consumed cycles, or an Error generated from
/// one script. It serves as an example on how to use the scheduler.
pub fn verify_tx(
    mock_tx: &MockTransaction,
    max_cycles: Cycle,
    cycles_per_iterate: Cycle,
    cycles_per_suspend: Cycle,
) -> Result<Cycle, Error> {
    let resource = Resource::from_both(mock_tx, DummyResourceLoader {}).expect("create resource");
    let resolved_tx = Arc::new(
        resolve_transaction(
            mock_tx.core_transaction(),
            &mut HashSet::new(),
            &resource,
            &resource,
        )
        .expect("resolving tx"),
    );

    let consensus = Arc::new(
        ConsensusBuilder::default()
            .hardfork_switch(HardForks::new_dev())
            .build(),
    );
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

    let mut consumed_cycles = 0;
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

        let mut scheduler = Scheduler::new(tx_data.clone(), verifier);
        let mut last_suspended_cycles = 0;

        loop {
            if scheduler.consumed_cycles() > max_cycles {
                return Err(Error::Unexpected(format!(
                    "{} of hash {:#x} runs out of max cycles! Consumed: {}, max: {}",
                    t,
                    hash,
                    scheduler.consumed_cycles(),
                    max_cycles,
                )));
            }

            if scheduler.consumed_cycles() - last_suspended_cycles >= cycles_per_suspend {
                // Perform a full suspend here.
                let state = scheduler.suspend().expect("suspend");
                log::debug!(
                    "{} of hash {:#x} suspended state size: {} bytes",
                    t,
                    hash,
                    state.size()
                );
                scheduler = {
                    let verifier = TransactionScriptsVerifier::new(
                        resolved_tx.clone(),
                        resource.clone(),
                        consensus.clone(),
                        tx_env.clone(),
                    );
                    Scheduler::resume(tx_data.clone(), verifier, state)
                };
                last_suspended_cycles = scheduler.consumed_cycles();
            }

            log::debug!(
                "Iterate {} of hash {:#x} with {} limit cycles",
                t,
                hash,
                cycles_per_iterate
            );
            match scheduler.run(RunMode::LimitCycles(cycles_per_iterate)) {
                Ok((exit_code, total_cycles)) => {
                    if exit_code != 0 {
                        return Err(Error::Unexpected(format!(
                            "Non-zero return code: {}",
                            exit_code
                        )));
                    }
                    consumed_cycles += total_cycles;
                    if total_cycles > max_cycles || consumed_cycles > max_cycles {
                        return Err(Error::Unexpected(format!(
                            "{} of hash {:#x} runs out of max cycles! Consumed: {}, total: {} max: {}",
                            t, hash, consumed_cycles, total_cycles, max_cycles,
                        )));
                    }
                    log::info!(
                        "{} of hash {:#x} terminates, exit code: {}, consumed cycles: {}",
                        t,
                        hash,
                        exit_code,
                        total_cycles
                    );
                    break;
                }
                Err(Error::CyclesExceeded) => (),
                Err(e) => {
                    return Err(Error::Unexpected(format!(
                        "{} of hash {:#x} encounters error: {:?}",
                        t, hash, e
                    )));
                }
            }
        }
    }

    Ok(consumed_cycles)
}

pub fn build_mock_tx(seed: u64, program: Bytes, data: dag::Data) -> MockTransaction {
    let mut rng = StdRng::seed_from_u64(seed);

    let code_type_script = random_script(&mut rng, ScriptHashType::Type);
    let code_dep = MockCellDep {
        cell_dep: CellDep::new_builder()
            .out_point(random_out_point(&mut rng))
            .dep_type(DepType::Code.into())
            .build(),
        output: CellOutput::new_builder()
            .type_(Some(code_type_script.clone()).pack())
            .build(),
        data: program,
        header: None,
    };

    let input_lock_script = Script::new_builder()
        .code_hash(code_type_script.calc_script_hash())
        .hash_type(ScriptHashType::Type.into())
        .build();
    let input_cell = MockInput {
        input: CellInput::new_builder()
            .previous_output(random_out_point(&mut rng))
            .build(),
        output: CellOutput::new_builder().lock(input_lock_script).build(),
        data: Bytes::default(),
        header: None,
    };

    let tx = TransactionBuilder::default()
        .cell_dep(code_dep.cell_dep.clone())
        .input(input_cell.input.clone())
        .output(CellOutput::new_builder().build())
        .witness(data.as_bytes().pack())
        .build();

    MockTransaction {
        tx: tx.data(),
        mock_info: MockInfo {
            inputs: vec![input_cell],
            cell_deps: vec![code_dep],
            header_deps: vec![],
            extensions: vec![],
        },
    }
}

pub fn generate_data_graph(
    seed: u64,
    spawns: u32,
    writes: u32,
    converging_threshold: u32,
) -> Result<dag::Data, Error> {
    let mut rng = StdRng::seed_from_u64(seed);

    let mut spawn_dag: Dag<(), ()> = Dag::new();
    let mut write_dag: Dag<(), ()> = Dag::new();

    // Root node denoting entrypoint VM
    let spawn_root = spawn_dag.add_node(());
    let write_root = write_dag.add_node(());
    assert_eq!(spawn_root.index(), 0);
    assert_eq!(write_root.index(), 0);

    let mut spawn_nodes = vec![spawn_root];
    let mut write_nodes = vec![write_root];

    for _ in 1..=spawns {
        let write_node = write_dag.add_node(());
        write_nodes.push(write_node);

        let previous_node = spawn_nodes[rng.gen_range(0..spawn_nodes.len())];
        let (_, spawn_node) = spawn_dag.add_child(previous_node, (), ());
        spawn_nodes.push(spawn_node);
    }

    let mut write_edges = Vec::new();
    if spawns > 0 {
        for _ in 1..=writes {
            let mut updated = false;

            for _ in 0..converging_threshold {
                let first_index = rng.gen_range(0..write_nodes.len());
                let second_index = {
                    let mut i = first_index;
                    while i == first_index {
                        i = rng.gen_range(0..write_nodes.len());
                    }
                    i
                };

                let first_node = write_nodes[first_index];
                let second_node = write_nodes[second_index];

                if let Ok(e) = write_dag.add_edge(first_node, second_node, ()) {
                    write_edges.push(e);
                    updated = true;
                    break;
                }
            }

            if !updated {
                break;
            }
        }
    }

    // Edge index -> pipe indices. Daggy::edge_endpoints helps us finding
    // nodes (vms) from edges (spawns)
    let mut spawn_ops: HashMap<usize, Vec<usize>> = HashMap::default();
    // Node index -> created pipes
    let mut pipes_ops: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::default();

    let mut spawn_edges = Vec::new();
    // Traversing spawn_dag for spawn operations
    let mut processing = VecDeque::from([spawn_root]);
    while !processing.is_empty() {
        let node = processing.pop_front().unwrap();
        pipes_ops.insert(node.index(), Vec::new());
        let children: Vec<_> = spawn_dag.children(node).iter(&spawn_dag).collect();
        for (e, n) in children.into_iter().rev() {
            spawn_ops.insert(e.index(), Vec::new());
            spawn_edges.push(e);

            processing.push_back(n);
        }
    }

    let mut writes_builder = dag::WritesBuilder::default();
    // Traversing all edges in write_dag
    for e in write_edges {
        let (writer, reader) = write_dag.edge_endpoints(e).unwrap();
        assert_ne!(writer, reader);
        let writer_pipe_index = e.index() * 2 + 1;
        let reader_pipe_index = e.index() * 2;

        // Generate finalized write op
        {
            let data_len = rng.gen_range(1..=1024);
            let mut data = vec![0u8; data_len];
            rng.fill(&mut data[..]);

            writes_builder = writes_builder.push(
                dag::WriteBuilder::default()
                    .from(build_vm_index(writer.index() as u64))
                    .from_pipe(build_pipe_index(writer_pipe_index as u64))
                    .to(build_vm_index(reader.index() as u64))
                    .to_pipe(build_pipe_index(reader_pipe_index as u64))
                    .data(
                        dag::BytesBuilder::default()
                            .extend(data.iter().map(|b| Byte::new(*b)))
                            .build(),
                    )
                    .build(),
            );
        }

        // Finding the lowest common ancestor of writer & reader nodes
        // in spawn_dag, which will creates the pair of pipes. Note that
        // all traversed spawn edges will have to pass the pipes down.
        //
        // TODO: we use a simple yet slow LCA solution, a faster algorithm
        // can be used to replace the code here if needed.
        let ancestor = {
            let mut a = writer;
            let mut b = reader;

            let mut set_a = HashSet::new();
            set_a.insert(a);
            let mut set_b = HashSet::new();
            set_b.insert(b);

            loop {
                let parents_a: Vec<_> = spawn_dag.parents(a).iter(&spawn_dag).collect();
                let parents_b: Vec<_> = spawn_dag.parents(b).iter(&spawn_dag).collect();

                assert!(
                    ((parents_a.len() == 1) && (parents_b.len() == 1))
                        || (parents_a.is_empty() && (parents_b.len() == 1))
                        || ((parents_a.len() == 1) && parents_b.is_empty())
                );

                // Update spawn ops to pass down pipes via edges, also update
                // each node's path node list
                if parents_a.len() == 1 {
                    let (_, parent_a) = parents_a[0];
                    set_a.insert(parent_a);

                    a = parent_a;
                }
                if parents_b.len() == 1 {
                    let (_, parent_b) = parents_b[0];
                    set_b.insert(parent_b);

                    b = parent_b;
                }

                // Test for ancestor
                if parents_a.len() == 1 {
                    let (_, parent_a) = parents_a[0];
                    if set_b.contains(&parent_a) {
                        break parent_a;
                    }
                }
                if parents_b.len() == 1 {
                    let (_, parent_b) = parents_b[0];
                    if set_a.contains(&parent_b) {
                        break parent_b;
                    }
                }
            }
        };

        // Update the path from each node to the LCA so we can pass created
        // pipes from LCA to each node
        {
            let mut a = writer;
            while a != ancestor {
                let parents_a: Vec<_> = spawn_dag.parents(a).iter(&spawn_dag).collect();
                assert!(parents_a.len() == 1);
                let (edge_a, parent_a) = parents_a[0];
                spawn_ops
                    .get_mut(&edge_a.index())
                    .unwrap()
                    .push(writer_pipe_index);
                a = parent_a;
            }

            let mut b = reader;
            while b != ancestor {
                let parents_b: Vec<_> = spawn_dag.parents(b).iter(&spawn_dag).collect();
                assert!(parents_b.len() == 1);
                let (edge_b, parent_b) = parents_b[0];
                spawn_ops
                    .get_mut(&edge_b.index())
                    .unwrap()
                    .push(reader_pipe_index);
                b = parent_b;
            }
        }

        // Create the pipes at the ancestor node
        pipes_ops
            .get_mut(&ancestor.index())
            .unwrap()
            .push((reader_pipe_index, writer_pipe_index));
    }

    let mut spawns_builder = dag::SpawnsBuilder::default();
    for e in spawn_edges {
        let (parent, child) = spawn_dag.edge_endpoints(e).unwrap();

        let pipes = {
            let mut builder = dag::PipeIndicesBuilder::default();
            for p in &spawn_ops[&e.index()] {
                builder = builder.push(build_pipe_index(*p as u64));
            }
            builder.build()
        };

        spawns_builder = spawns_builder.push(
            dag::SpawnBuilder::default()
                .from(build_vm_index(parent.index() as u64))
                .child(build_vm_index(child.index() as u64))
                .pipes(pipes)
                .build(),
        );
    }

    let mut pipes_builder = dag::PipesBuilder::default();
    for (vm_index, pairs) in pipes_ops {
        for (reader_pipe_index, writer_pipe_index) in pairs {
            pipes_builder = pipes_builder.push(
                dag::PipeBuilder::default()
                    .vm(build_vm_index(vm_index as u64))
                    .read_pipe(build_pipe_index(reader_pipe_index as u64))
                    .write_pipe(build_pipe_index(writer_pipe_index as u64))
                    .build(),
            );
        }
    }

    Ok(dag::DataBuilder::default()
        .spawns(spawns_builder.build())
        .pipes(pipes_builder.build())
        .writes(writes_builder.build())
        .build())
}

fn build_vm_index(val: u64) -> dag::VmIndex {
    let mut data = [Byte::new(0); 8];
    for (i, v) in val.to_le_bytes().into_iter().enumerate() {
        data[i] = Byte::new(v);
    }
    dag::VmIndexBuilder::default().set(data).build()
}

fn build_pipe_index(val: u64) -> dag::PipeIndex {
    let mut data = [Byte::new(0); 8];
    for (i, v) in val.to_le_bytes().into_iter().enumerate() {
        data[i] = Byte::new(v);
    }
    dag::PipeIndexBuilder::default().set(data).build()
}

fn random_out_point<R: Rng>(rng: &mut R) -> OutPoint {
    let tx_hash = {
        let mut buf = [0u8; 32];
        rng.fill(&mut buf);
        buf.pack()
    };
    OutPoint::new(tx_hash, 0)
}

pub fn random_script<R: Rng>(rng: &mut R, t: ScriptHashType) -> Script {
    let code_hash = {
        let mut buf = [0u8; 32];
        rng.fill(&mut buf[..]);
        buf.pack()
    };
    let args = {
        let len = rng.gen_range(1..101);
        let mut buf = vec![0u8; len];
        rng.fill(&mut buf[..]);
        buf.pack()
    };
    Script::new_builder()
        .code_hash(code_hash)
        .hash_type(t.into())
        .args(args)
        .build()
}
