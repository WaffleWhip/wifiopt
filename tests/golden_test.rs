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
