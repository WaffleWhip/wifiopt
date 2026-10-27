use wifiopt::db::DiscoverRow;
use wifiopt::{discover, graph};

fn row(json_config: &str) -> DiscoverRow {
    DiscoverRow {
        serial: "SN-M".to_string(),
        channel_ont: None,
        channel_ont5g: None,
        json_config: json_config.to_string(),
        json_neighbor: "{}".to_string(),
    }
}

fn cfg5(slot: u8, channel: &str) -> String {
    format!(
        "\"{slot}\":{{\"ssid_enable\":true,\"channel\":{channel},\"bssid\":\"aa:bb:cc:dd:ee:{slot:02x}\",\"possible_channel\":\"36,40,44,48,149,153,157,161\"}}"
    )
}

#[test]
fn primary_is_lowest_slot_regardless_of_json_key_order() {
    // "6" written before "5": old HashMap code could read either radio.
    let ap = discover::parse(&row(&format!(
        "{{{},{}}}",
        cfg5(6, "149"),
        cfg5(5, "48")
    )))
    .unwrap();
    assert_eq!(ap.radios_5g.len(), 2);
    assert_eq!(ap.radios_5g[0].slot, 5);
    assert_eq!(ap.radios_5g[1].slot, 6);
    let mem = graph::mem_rows_for(&[ap], "5g");
    assert_eq!(mem[0].own_channel, Some(48));
}

#[test]
fn slots_sort_numerically_not_lexicographically() {
    // BTreeMap<String> orders "10" before "5"; numeric sort must win.
    let ap = discover::parse(&row(&format!(
        "{{{},{}}}",
        cfg5(10, "149"),
        cfg5(5, "48")
    )))
    .unwrap();
    assert_eq!(ap.radios_5g[0].slot, 5);
    let mem = graph::mem_rows_for(&[ap], "5g");
    assert_eq!(mem[0].own_channel, Some(48));
}

#[test]
fn empty_channel_primary_falls_through_to_next_valid() {
    // FHTT9D2E0440 shape: slot 5 ok on some days, empty sibling radios.
    let ap = discover::parse(&row(&format!(
        "{{{},{}}}",
        cfg5(5, "\"\""),
        cfg5(6, "149")
    )))
    .unwrap();
    let mem = graph::mem_rows_for(&[ap], "5g");
    assert_eq!(mem[0].own_channel, Some(149));
}

#[test]
fn out_of_band_channel_on_lowest_slot_is_skipped() {
    // 48575443F9327CAE shape: slot 6 reports 2G channel 11.
    let ap = discover::parse(&row(&format!(
        "{{{},{}}}",
        "\"5\":{\"ssid_enable\":true,\"channel\":48,\"bssid\":\"aa:bb:cc:dd:ee:05\",\"possible_channel\":\"36,40,44,48\"}",
        "\"6\":{\"ssid_enable\":true,\"channel\":11,\"bssid\":\"aa:bb:cc:dd:ee:06\",\"possible_channel\":\"1,2,3,4,5,6,7,8,9,10,11,12,13\"}"
    )))
    .unwrap();
    assert!(!discover::divergent_radios(&ap, "2g"));
    assert!(discover::divergent_radios(&ap, "5g"));
    let mem = graph::mem_rows_for(&[ap], "5g");
    assert_eq!(mem[0].own_channel, Some(48));
}

#[test]
fn single_radio_is_not_divergent() {
    let ap = discover::parse(&row(&format!("{{\"5\":{}}}", "{ \"ssid_enable\":true,\"channel\":48,\"bssid\":\"aa:bb:cc:dd:ee:05\",\"possible_channel\":\"36,40,44,48\" }")))
        .unwrap();
    assert!(!discover::divergent_radios(&ap, "5g"));
    let mem = graph::mem_rows_for(&[ap], "5g");
    assert_eq!(mem[0].own_channel, Some(48));
}

#[test]
fn no_enabled_radios_gives_none_channel() {
    let ap = discover::parse(&row(
        "{\"5\":{\"ssid_enable\":false,\"channel\":48,\"bssid\":\"aa:bb:cc:dd:ee:05\",\"possible_channel\":\"36,40,44,48\"}}",
    ))
    .unwrap();
    assert!(ap.radios_5g.is_empty());
    let mem = graph::mem_rows_for(&[ap], "5g");
    assert_eq!(mem[0].own_channel, None);
    assert!(mem[0].own.is_empty());
}
