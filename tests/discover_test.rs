use wifiopt::db::DiscoverRow;
use wifiopt::discover;

fn row() -> DiscoverRow {
    DiscoverRow {
        serial: "SN123".to_string(),
        channel_ont: Some(6),
        channel_ont5g: Some(149),
        json_config: r#"{"1":{"ssid_enable":true,"channel":6,"bssid":"AA:BB:CC:DD:EE:01","possible_channel":"1,2,3,4,5,6,7,8,9,10,11,12,13"},"5":{"ssid_enable":true,"channel":149,"bssid":"AA:BB:CC:DD:EE:02","possible_channel":"36,40,44,48,149,153,157,161"}}"#.to_string(),
        json_neighbor: r#"{"1":{"ssid":"n1","bssid":"11:22:33:44:55:66","channel":"1","signal_strength":"-81"},"2":{"ssid":"n2","bssid":"AA:BB:CC:DD:EE:09","channel":149,"signal_strength":"-70"}}"#.to_string(),
    }
}

#[test]
fn parses_slots_into_bands() {
    let ap = discover::parse(&row()).unwrap();
    assert_eq!(ap.radios_2g.len(), 1);
    assert_eq!(ap.radios_2g[0].channel, Some(6));
    assert_eq!(ap.radios_2g[0].possible.len(), 13);
    assert_eq!(ap.radios_5g.len(), 1);
    assert_eq!(ap.radios_5g[0].channel, Some(149));
    assert_eq!(ap.neighbors_2g.len(), 1);
    assert_eq!(ap.neighbors_2g[0].signal, -81);
    assert_eq!(ap.neighbors_5g.len(), 1);
}

#[test]
fn disabled_ssid_empty_channel_does_not_drop_row() {
    let r = DiscoverRow {
        serial: "SNX".to_string(),
        channel_ont: Some(11),
        channel_ont5g: None,
        json_config: r#"{"1":{"ssid_enable":true,"channel":11,"bssid":"aa:11","possible_channel":"1,2,3,4,5,6,7,8,9,10,11,12,13"},"2":{"ssid_enable":false,"channel":"","ssid_name":"","possible_channel":""}}"#.to_string(),
        json_neighbor: "[]".to_string(),
    };
    let ap = discover::parse(&r).unwrap();
    assert_eq!(ap.radios_2g.len(), 1);
    assert_eq!(ap.radios_2g[0].channel, Some(11));
}

#[test]
fn one_bad_neighbor_entry_empties_map() {
    let mut r = row();
    r.json_neighbor = r#"{"1":{"bssid":"11:22:33:44:55:66","channel":"x","signal_strength":"y"}}"#.to_string();
    let ap = discover::parse(&r).unwrap();
    assert!(ap.neighbors_2g.is_empty());
}

#[test]
fn empty_neighbor_and_missing_possible_falls_back() {
    let mut r = row();
    r.json_neighbor = "[]".to_string();
    r.json_config = r#"{"1":{"ssid_enable":true,"channel":3,"bssid":"AA:01","possible_channel":""}}"#.to_string();
    let ap = discover::parse(&r).unwrap();
    assert!(ap.neighbors_2g.is_empty());
    assert!(ap.radios_2g[0].possible.is_empty());
}
