use wifiopt::db::DiscoverRow;
use wifiopt::discover;

fn row() -> DiscoverRow {
    DiscoverRow {
        serial: "SN123".to_string(),
        channel_ont: Some(6),
        channel_ont5g: Some(149),
        json_config: r#"{"1":{"ssid_enable":true,"channel":6,"bssid":"AA:BB:CC:DD:EE:01","possible_channel":"1,2,3,4,5,6,7,8,9,10,11,12,13"},"5":{"ssid_enable":true,"channel":149,"bssid":"AA:BB:CC:DD:EE:02","possible_channel":"36,40,44,48,149,153,157,161"}}"#.to_string(),
        json_neighbor: r#"{"1":{"ssid":"n1","bssid":"11:22:33:44:55:66","channel":"1","signal_strength":"-81"},"2":{"ssid":"bad","bssid":"00:00:00:00:00:00","channel":"x","signal_strength":"y"}}"#.to_string(),
    }
}

#[test]
fn parses_config_and_neighbors() {
    let ap = discover::parse(&row()).unwrap();
    assert_eq!(ap.band2g.as_ref().unwrap().current, 6);
    assert_eq!(ap.band2g.as_ref().unwrap().possible.len(), 13);
    assert_eq!(ap.band5g.as_ref().unwrap().current, 149);
    assert_eq!(ap.neighbors.len(), 1);
    assert_eq!(ap.neighbors[0].signal, -81);
}

#[test]
fn empty_neighbor_and_missing_possible_falls_back() {
    let mut r = row();
    r.json_neighbor = "[]".to_string();
    r.json_config = r#"{"1":{"channel":3,"possible_channel":""}}"#.to_string();
    let ap = discover::parse(&r).unwrap();
    assert!(ap.neighbors.is_empty());
    assert_eq!(ap.band2g.as_ref().unwrap().possible.len(), 13);
}
