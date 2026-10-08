use std::collections::HashMap;
use wifiopt::db::DiscoverRow;
use wifiopt::discover;
use wifiopt::output;
use wifiopt::pipeline;

fn row(sn: &str, ch: u8) -> DiscoverRow {
    DiscoverRow {
        serial: sn.to_string(),
        channel_ont: Some(ch),
        channel_ont5g: None,
        json_config: format!(
            "{{\"1\":{{\"channel\":{ch},\"possible_channel\":\"1,6,11\"}}}}"
        ),
        json_neighbor: "[]".to_string(),
    }
}

fn row2(sn: &str, ch: u8, nbr_bssid: &str, nbr_ch: &str) -> DiscoverRow {
    DiscoverRow {
        serial: sn.to_string(),
        channel_ont: Some(ch),
        channel_ont5g: None,
        json_config: format!(
            "{{\"1\":{{\"channel\":{ch},\"possible_channel\":\"1,6,11\"}}}}"
        ),
        json_neighbor: format!(
            "{{\"1\":{{\"ssid\":\"n\",\"bssid\":\"{nbr_bssid}\",\"channel\":\"{nbr_ch}\",\"signal_strength\":\"-70\"}}}}"
        ),
    }
}

#[test]
fn golden_fixture_matches_shape() {
    let rows = vec![row("S1", 6), row("S2", 1)];
    let aps: Vec<_> = rows.iter().map(|r| discover::parse(r).unwrap()).collect();
    let out = pipeline::optimize("reg1", "20261007", &aps, &HashMap::new(), -85.0, 0.05);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].reg, "reg1");
    assert_eq!(output::header().split(',').count(), 23);
    assert!(out[0].status_2g.is_some());
    assert!(out[0].clusterid_2g.is_some());
}

#[test]
fn cluster_mates_avoid_same_channel() {
    let rows = vec![
        row2("S1", 6, "BB:BB:BB:BB:BB:02", "6"),
        row2("S2", 6, "AA:AA:AA:AA:AA:01", "6"),
    ];
    let aps: Vec<_> = rows.iter().map(|r| discover::parse(r).unwrap()).collect();
    let mut bssid = HashMap::new();
    bssid.insert("AA:AA:AA:AA:AA:01".to_string(), "S1".to_string());
    bssid.insert("BB:BB:BB:BB:BB:02".to_string(), "S2".to_string());
    let out = pipeline::optimize("reg1", "20261007", &aps, &bssid, -85.0, 0.05);
    assert_ne!(out[0].ch_after_2g, out[1].ch_after_2g);
}
