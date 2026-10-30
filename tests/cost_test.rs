use wifiopt::cost::{
    accept_move, band_cost_ok, overlap_2g, overlap_5g, optimize_band, possible_channels,
    rssi_factor,
};
use wifiopt::graph::MemRow;

fn mem(sn: &str, own: Vec<&str>, own_ch: Option<i32>, possible: Vec<i32>, nb: Vec<(&str, i32, i32)>) -> MemRow {
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

#[test]
fn overlap_values_match_spectral_model() {
    assert_eq!(overlap_2g(6, 6), 1.0);
    assert_eq!(overlap_2g(6, 7), 0.75);
    assert_eq!(overlap_2g(6, 8), 0.5);
    assert_eq!(overlap_2g(1, 6), 0.0);
    assert_eq!(overlap_5g(149, 149), 0.625);
}

#[test]
fn rssi_buckets() {
    assert_eq!(rssi_factor(-45), 1.0);
    assert_eq!(rssi_factor(-55), 0.8);
    assert_eq!(rssi_factor(-65), 0.6);
    assert_eq!(rssi_factor(-75), 0.3);
    assert_eq!(rssi_factor(-90), 0.1);
}

#[test]
fn candidates_restricted_and_dfs_stripped() {
    let m = mem("A", vec!["aa:01"], Some(6), vec![1, 2, 3, 6, 11], vec![]);
    assert_eq!(possible_channels(&m, "2g"), vec![1, 6, 11]);
    let m5 = mem(
        "A",
        vec!["aa:05"],
        Some(149),
        vec![52, 149, 153],
        vec![],
    );
    assert_eq!(possible_channels(&m5, "5g"), vec![149, 153]);
}

#[test]
fn readme_example_moves_6_to_1() {
    let m = mem(
        "485754432288A0B6",
        vec!["9c:bf:cd:2d:22:84"],
        Some(6),
        vec![1, 6, 11],
        vec![
            ("ext:01", 6, -53),
            ("ext:02", 6, -70),
            ("ext:03", 6, -78),
            ("ext:04", 7, -66),
            ("ext:05", 7, -70),
        ],
    );
    let out = optimize_band(&[m], &[], 50, "2g");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].ch_after, 1);
    assert_eq!(out[0].status, "CHANGE");
    assert!(out[0].cost_after < 0.001);
}

#[test]
fn band_cost_ok_filters() {
    assert!(band_cost_ok(6, "2g"));
    assert!(band_cost_ok(14, "2g"));
    assert!(band_cost_ok(149, "5g"));
    assert!(!band_cost_ok(52, "5g"));
}

#[test]
fn accept_move_reverts_final_regression() {
    // worse beyond noise -> stay
    assert_eq!(accept_move(1, 1.0, 6, 1.5, true), (1, 1.0));
    assert_eq!(accept_move(1, 1.0, 6, 1.0006, true), (1, 1.0));
    // better or within noise -> keep
    assert_eq!(accept_move(1, 1.0, 6, 0.5, true), (6, 0.5));
    assert_eq!(accept_move(1, 1.0, 6, 1.0005, true), (6, 1.0005));
    // no move -> untouched
    assert_eq!(accept_move(6, 0.5, 6, 0.5, true), (6, 0.5));
}

#[test]
fn accept_move_never_keeps_offplan_channel() {
    // non-plan 2.4G channel (5) must canonicalize even when pricier
    assert_eq!(accept_move(5, 0.045, 1, 0.9, false), (1, 0.9));
    // DFS 5G channel (56) must leave DFS even when pricier
    assert_eq!(accept_move(56, 0.21, 36, 0.8, false), (36, 0.8));
}

#[test]
fn optimize_never_reports_worse_change() {
    // mutual interferers: sequential moves can strand one AP worse;
    // the final guard must convert those into STAY.
    let a = mem(
        "A",
        vec!["aa:01"],
        Some(6),
        vec![1, 6, 11],
        vec![("aa:02", 6, -50)],
    );
    let b = mem(
        "B",
        vec!["aa:02"],
        Some(6),
        vec![1, 6, 11],
        vec![("aa:01", 6, -50)],
    );
    let out = optimize_band(&[a, b], &[], 50, "2g");
    assert_eq!(out.len(), 2);
    for r in &out {
        if r.status == "CHANGE" {
            assert!(
                r.cost_after <= r.cost_before + 0.0005,
                "worse CHANGE slipped through: {r:?}"
            );
        } else {
            assert_eq!(r.ch_after, r.ch_before.unwrap());
        }
    }
}
