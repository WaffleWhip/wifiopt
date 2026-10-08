use std::collections::HashMap;
use wifiopt::discover::{ApInfo, Neighbor, RadioInfo};
use wifiopt::graph;

fn ap(serial: &str, _bssid: &str, nbrs: Vec<Neighbor>) -> ApInfo {
    ApInfo {
        serial: serial.to_string(),
        band2g: Some(RadioInfo {
            current: 6,
            possible: vec![1, 6, 11],
        }),
        band5g: None,
        neighbors: nbrs,
    }
}

fn nb(bssid: &str, channel: u8, signal: i16) -> Neighbor {
    Neighbor {
        bssid: bssid.to_string(),
        channel,
        signal,
    }
}

fn chain() -> (Vec<ApInfo>, HashMap<String, String>) {
    let aps = vec![
        ap("A", "AA:01", vec![nb("BB:02", 6, -70)]),
        ap("B", "BB:02", vec![nb("AA:01", 6, -70), nb("CC:03", 6, -70)]),
        ap("C", "CC:03", vec![nb("BB:02", 6, -70)]),
    ];
    let mut m = HashMap::new();
    m.insert("AA:01".to_string(), "A".to_string());
    m.insert("BB:02".to_string(), "B".to_string());
    m.insert("CC:03".to_string(), "C".to_string());
    (aps, m)
}

#[test]
fn connected_aps_share_cluster() {
    let (aps, idx) = chain();
    let c = graph::clusters(&aps, &idx, -85.0);
    assert_eq!(c.g2.len(), 1);
    assert_eq!(c.g2[0].len(), 3);
}

#[test]
fn weak_signal_splits_cluster() {
    let (aps, idx) = chain();
    let c = graph::clusters(&aps, &idx, -60.0);
    assert_eq!(c.g2.len(), 3);
}
