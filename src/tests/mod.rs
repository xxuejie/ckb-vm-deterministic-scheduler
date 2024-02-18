#[allow(dead_code)]
mod dag;

use ckb_vm::{Bytes, Error};
use daggy::{Dag, Walker};
use molecule::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub fn generate_data_graph(
    seed: u64,
    spawns: u32,
    writes: u32,
    converging_threshold: u32,
) -> Result<Bytes, Error> {
    let mut rng = StdRng::seed_from_u64(seed);

    let mut spawn_dag: Dag<(), ()> = Dag::new();
    let mut write_dag: Dag<(), ()> = Dag::new();

    // Root node denoting entrypoint VM
    let spawn_root = spawn_dag.add_node(());
    let write_root = write_dag.add_node(());

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
                let first_index = rng.gen_range(0..=write_nodes.len());
                let second_index = {
                    let mut i = first_index;
                    while i == first_index {
                        i = rng.gen_range(0..=write_nodes.len());
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
        for (e, n) in spawn_dag.children(node).iter(&spawn_dag) {
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
                    (parents_a.len() == 1) && (parents_b.len() == 1)
                        || (parents_a.len() == 0) && (parents_b.len() == 1)
                        || (parents_a.len() == 1) && (parents_b.len() == 0)
                );

                // Update spawn ops to pass down pipes via edges, also update
                // each node's path node list
                if parents_a.len() == 1 {
                    let (edge_a, parent_a) = parents_a[0];
                    spawn_ops
                        .get_mut(&edge_a.index())
                        .unwrap()
                        .push(writer_pipe_index);
                    set_a.insert(parent_a);

                    a = parent_a;
                }
                if parents_b.len() == 1 {
                    let (edge_b, parent_b) = parents_b[1];
                    spawn_ops
                        .get_mut(&edge_b.index())
                        .unwrap()
                        .push(reader_pipe_index);
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

    let data = dag::DataBuilder::default()
        .spawns(spawns_builder.build())
        .pipes(pipes_builder.build())
        .writes(writes_builder.build())
        .build();

    Ok(data.as_bytes())
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
