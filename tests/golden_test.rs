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
            "{{\"1\":{{\"ssid_enable\":true,\"channel\":{ch},\"bssid\":\"aa:{sn}\",\"possible_channel\":\"1,6,11\"}}}}"
        ),
        json_neighbor: "[]".to_string(),
    }
}

#[test]
fn golden_fixture_matches_shape() {
    let rows = vec![row("S1", 6), row("S2", 1)];
    let aps: Vec<_> = rows.iter().map(|r| discover::parse(r).unwrap()).collect();
    let out = pipeline::optimize("reg1", aps, 50, false, 4);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].reg, "reg1");
    assert_eq!(output::header().split(',').count(), 23);
    assert!(out[0].status_2g.is_some());
    assert!(out[0].clusterid_2g.is_none());
}

#[test]
fn ap_without_2g_radio_absent_from_output() {
    let r = DiscoverRow {
        serial: "S5".to_string(),
        channel_ont: None,
        channel_ont5g: Some(149),
        json_config: r#"{"5":{"ssid_enable":true,"channel":149,"bssid":"aa:s5","possible_channel":"36,149"}}"#.to_string(),
        json_neighbor: "[]".to_string(),
    };
    let aps = vec![discover::parse(&r).unwrap()];
    let out = pipeline::optimize("reg1", aps, 50, false, 4);
    assert!(out.is_empty());
    let _ = HashMap::<String, String>::new();
}
