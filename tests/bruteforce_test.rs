use wifiopt::cost::{optimize_band, optimize_band_bruteforce};
use wifiopt::graph::MemRow;

fn mem(
    sn: &str,
    own: Vec<&str>,
    own_ch: Option<i32>,
    possible: Vec<i32>,
    nb: Vec<(&str, i32, i32)>,
) -> MemRow {
    MemRow {
        sn: sn.to_string(),
        own: own.into_iter().map(|s| s.to_string()).collect(),
        neighbors: nb
            .into_iter()
            .map(|(b, c, r)| (b.to_string(), c, r))
            .collect(),
        own_channel: own_ch,
        possible_channels: possible,
    }
}

fn total_after(rows: &[wifiopt::cost::OptimizeRow]) -> f64 {
    rows.iter().map(|r| r.cost_after).sum()
}

// Two coupled APs + external anchors on both sides.
fn pair_fixture() -> (Vec<MemRow>, Vec<Vec<String>>) {
    let a = mem(
        "A",
        vec!["aa:01"],
        Some(36),
        vec![36, 40, 44, 48],
        vec![("aa:02", 36, -50), ("ext1", 48, -50)],
    );
    let b = mem(
        "B",
        vec!["aa:02"],
        Some(36),
        vec![36, 40, 44, 48],
        vec![("aa:01", 36, -50), ("ext2", 36, -50)],
    );
    (
        vec![a, b],
        vec![vec!["A".to_string(), "B".to_string()]],
    )
}

// Three-AP chain with two candidate channels.
fn chain_fixture() -> (Vec<MemRow>, Vec<Vec<String>>) {
    let a = mem(
        "A",
        vec!["aa:01"],
        Some(36),
        vec![36, 48],
        vec![("aa:02", 36, -60)],
    );
    let b = mem(
        "B",
        vec!["aa:02"],
        Some(36),
        vec![36, 48],
        vec![("aa:01", 36, -60), ("aa:03", 36, -70)],
    );
    let c = mem(
        "C",
        vec!["aa:03"],
        Some(48),
        vec![36, 48],
        vec![("aa:02", 36, -70), ("ext9", 48, -55)],
    );
    (
        vec![a, b, c],
        vec![vec!["A".to_string(), "B".to_string(), "C".to_string()]],
    )
}

#[test]
fn joint_total_never_worse_than_descent() {
    for (rows, clusters) in [pair_fixture(), chain_fixture()] {
        let (bf, _) = optimize_band_bruteforce(&rows, &clusters, 50, 4, "5g");
        let desc = optimize_band(&rows, &clusters, 50, "5g");
        assert_eq!(bf.len(), desc.len());
        assert!(
            total_after(&bf) <= total_after(&desc) + 1e-9,
            "bruteforce must be exact-optimal: bf={} desc={}",
            total_after(&bf),
            total_after(&desc)
        );
    }
}

#[test]
fn bruteforce_is_deterministic() {
    let (rows, clusters) = pair_fixture();
    let (bf1, _) = optimize_band_bruteforce(&rows, &clusters, 50, 4, "5g");
    let (bf2, _) = optimize_band_bruteforce(&rows, &clusters, 50, 4, "5g");
    for (a, b) in bf1.iter().zip(bf2.iter()) {
        assert_eq!((&a.sn, a.ch_after), (&b.sn, b.ch_after));
    }
}

#[test]
fn singleton_matches_descent_exactly() {
    let m = mem("S", vec!["aa:09"], Some(149), vec![36, 48, 149], vec![("ext", 36, -70)]);
    let (bf, stats) = optimize_band_bruteforce(&[m.clone()], &[], 50, 4, "5g");
    let desc = optimize_band(&[m], &[], 50, "5g");
    assert_eq!(stats.groups, 1);
    assert_eq!(stats.exhaustive, 1);
    assert_eq!(stats.fallback, 0);
    assert_eq!(bf[0].ch_after, desc[0].ch_after);
    assert_eq!(bf[0].status, desc[0].status);
}

#[test]
fn oversize_group_falls_back_with_invariant() {
    let sns: Vec<String> = (0..5).map(|i| format!("M{i}")).collect();
    let bss: Vec<String> = (0..5).map(|i| format!("bb:{i:02}")).collect();
    let rows: Vec<MemRow> = sns
        .iter()
        .zip(bss.iter())
        .map(|(sn, b)| {
            mem(
                sn,
                vec![b.as_str()],
                Some(36),
                vec![36, 48],
                vec![],
            )
        })
        .collect();
    let (bf, stats) = optimize_band_bruteforce(&rows, &[sns], 50, 4, "5g");
    assert_eq!(stats.fallback, 1);
    assert_eq!(stats.exhaustive, 0);
    assert_eq!(bf.len(), 5);
    for r in &bf {
        if r.status == "CHANGE" {
            assert!(r.cost_after <= r.cost_before + 0.0005);
        }
    }
}
