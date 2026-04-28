use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ghost_brain::security::cabal_detector::{
    CabalDetectorConfig, HolderProfile, SecurityEngine, TokenContext, Verdict,
};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};

fn mock_pubkey(byte: u8) -> Pubkey {
    let mut bytes = [0u8; 32];
    bytes[0] = byte;
    Pubkey::new_from_array(bytes)
}

fn create_worst_case_context(holders_count: usize, _max_depth: u8) -> TokenContext {
    let mut holders = Vec::with_capacity(holders_count);
    let mut funding_graph = HashMap::new();
    let root = mock_pubkey(255);

    // Realistic Worst Case: "Graph Explosion" (Tree Structure)
    // Root -> 2 Children -> 4 Children -> ...
    // This forces cache misses in the hashmap as we traverse different branches.

    // We build a binary tree of funders.
    // Nodes are indexed 0..N.
    // 0 is root.
    // Children of i are 2*i + 1, 2*i + 2.

    // We map integer indices to Pubkeys.
    let index_to_pubkey = |i: usize| -> Pubkey {
        let mut bytes = [0u8; 32];
        // Simple scattering to avoid trivial sequential access patterns if hashmap is bad execution
        // (though standard Hasher handles this)
        let val = (i as u64).to_le_bytes();
        bytes[0..8].copy_from_slice(&val);
        Pubkey::new_from_array(bytes)
    };

    // Build tree layer by layer until we have enough leaves for holders?
    // Or just valid path for each holder up to Depth 5?

    // Let's create a shared massive graph where paths merge.
    // Holders connect to leaf nodes of a depth-5 tree.
    // Depth 5 binary tree has 31 nodes (1+2+4+8+16).
    // Let's make it bigger to ensure cache pressure. Depth 10 => 1024 leaves.

    let depth = 8; // 256 leaves. 50 holders will pick from them.

    // Populate graph edges
    let max_node_index = (1 << (depth + 1)) - 1;
    for i in 1..=max_node_index {
        let parent_idx = (i - 1) / 2;
        let p_key = index_to_pubkey(parent_idx);
        let c_key = index_to_pubkey(i);
        funding_graph.insert(c_key, p_key);
    }

    // Assign holders to random leaves
    // Holder i connects to node (2^depth - 1) + i
    // This makes them start deep in the tree.
    let leaf_start_index = (1 << depth) - 1;

    for i in 0..holders_count {
        let leaf_idx = leaf_start_index + (i % (1 << depth));
        let leaf_key = index_to_pubkey(leaf_idx);

        holders.push(HolderProfile {
            address: mock_pubkey(i as u8), // Holder own address
            balance: 100_000,
            funding_source: Some(leaf_key), // Points to a leaf in the graph
            first_buy_slot: 1000 + (i as u64 % 5), // Spread across few slots
            compute_unit_limit: 200_000 + (i as u32 % 2), // 2 groups of settings
            priority_fee_lamports: 5000 + (i as u64 % 2),
        });
    }

    TokenContext {
        mint_address: mock_pubkey(0),
        total_supply: (holders_count * 100_000 * 5) as u64, // ~20% supply
        holders,
        known_exchange_addresses: HashSet::new(),
        funding_graph,
    }
}

fn bench_security_engine(c: &mut Criterion) {
    let config = CabalDetectorConfig::default();
    let engine = SecurityEngine::new(config);

    // Worst Case Scenario:
    // - 50 Holders
    // - Max Recursion Depth (5)
    // - Cluster present (logic must traverse graph)
    // - Sniper bundle present (logic must count slots)
    // - Bot farm present (logic must hash params)
    let context = create_worst_case_context(50, 5);

    c.bench_function("cabal_detector_worst_case_50_holders", |b| {
        b.iter(|| {
            // We expect Reject, but we measure time to reach verdict
            let result = engine.evaluate_token_security(black_box(&context));
            // Ensure optimization doesn't remove the call (result is used implicitly by black_box return check config?)
            // Actually black_box(result) is better
            black_box(result);
        })
    });
}

criterion_group!(benches, bench_security_engine);
criterion_main!(benches);
