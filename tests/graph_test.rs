use wifiopt::db::DiscoverRow;
use wifiopt::discover;
use wifiopt::graph;

fn ap(sn: &str, bssid: &str, nbr_bssid: &str) -> wifiopt::discover::ApInfo {
    discover::parse(&DiscoverRow {
        serial: sn.to_string(),
        channel_ont: Some(6),
        channel_ont5g: None,
        json_config: format!(
            "{{\"1\":{{\"ssid_enable\":true,\"channel\":6,\"bssid\":\"{bssid}\",\"possible_channel\":\"1,6,11\"}}}}"
        ),
        json_neighbor: format!(
            "{{\"1\":{{\"ssid\":\"n\",\"bssid\":\"{nbr_bssid}\",\"channel\":\"6\",\"signal_strength\":\"-70\"}}}}"
        ),
    })
    .unwrap()
}

#[test]
fn managed_neighbors_share_cluster() {
    let aps = vec![
        ap("A", "aa:01", "bb:02"),
        ap("B", "bb:02", "aa:01"),
        ap("C", "cc:03", "ff:ff:ff:ff:ff:ff"),
    ];
    let c = graph::cluster(&aps, "2g", 2).unwrap();
    assert_eq!(c.valid.len(), 1);
    assert_eq!(c.valid[0].len(), 2);
}

#[test]
fn singletons_filtered_by_min_size() {
    let aps = vec![ap("A", "aa:01", "ff:ff:ff:ff:ff:ff")];
    let c = graph::cluster(&aps, "2g", 2).unwrap();
    assert!(c.valid.is_empty());
}
