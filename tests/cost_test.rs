use wifiopt::cost::{assign, Band};
use wifiopt::discover::{ApInfo, Neighbor, RadioInfo};

fn ap2g(serial: &str, current: u8, nbrs: Vec<Neighbor>) -> ApInfo {
    ApInfo {
        serial: serial.to_string(),
        band2g: Some(RadioInfo {
            current,
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

#[test]
fn picks_minimum_cost_channel() {
    let cluster = vec![
        ap2g("A", 6, vec![nb("X", 6, -70)]),
        ap2g("B", 6, vec![nb("Y", 1, -70)]),
    ];
    let a = assign(&cluster, Band::G2, -85.0, 0.05);
    assert_eq!(a.len(), 2);
    assert!(a[0].cost_after.unwrap() <= a[0].cost_before.unwrap());
}

#[test]
fn absent_band_yields_no_assignment() {
    let cluster = vec![ap2g("A", 6, vec![])];
    let a = assign(&cluster, Band::G5, -85.0, 0.05);
    assert!(a.is_empty());
}

#[test]
fn empty_neighbors_stay() {
    let cluster = vec![ap2g("A", 6, vec![])];
    let a = assign(&cluster, Band::G2, -85.0, 0.05);
    assert_eq!(a[0].status, "STAY");
}
