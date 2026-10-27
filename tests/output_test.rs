use wifiopt::output::{header, write_csv, write_summary, OutputRow, Summary};

fn row() -> OutputRow {
    OutputRow {
        reg: "reg1".to_string(),
        sn: "SN1".to_string(),
        clusterid_2g: Some(1),
        ch_before_2g: Some(9),
        cost_before_2g: Some(1.8),
        ch_after_2g: Some(1),
        cost_after_2g: Some(0.36),
        status_2g: Some("CHANGE".to_string()),
        clusterid_5g: None,
        ch_before_5g: None,
        cost_before_5g: None,
        ch_after_5g: None,
        cost_after_5g: None,
        status_5g: None,
        updated_at: "2026-10-07 20:00:20".to_string(),
        node_id: None,
        node_ip: None,
        slot: None,
        port: None,
        onu_id: None,
        regional: None,
        witel: None,
        cpe_product_class: None,
    }
}

#[test]
fn csv_header_matches_wifi_optimize() {
    assert_eq!(
        header(),
        "reg,sn,clusterid_2g,ch_before_2g,cost_before_2g,ch_after_2g,cost_after_2g,status_2g,clusterid_5g,ch_before_5g,cost_before_5g,ch_after_5g,cost_after_5g,status_5g,updated_at,node_id,node_ip,slot,port,onu_id,regional,witel,cpe_product_class"
    );
    assert_eq!(header().split(',').count(), 23);
}

#[test]
fn csv_roundtrip_with_nulls() {
    // target/ is always writable; system temp varies by machine/env.
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("wifiopt_out_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("r.csv");
    let p = path.to_str().unwrap();
    write_csv(p, &[row()]).unwrap();
    let back = std::fs::read_to_string(p).unwrap();
    assert!(back.contains("SN1"));
    assert!(back.contains("CHANGE"));
    let summary = Summary {
        date: "20261007".to_string(),
        regs: vec!["reg1".to_string()],
        rows: 1,
    };
    let sp = dir.join("summary.json").to_str().unwrap().to_string();
    write_summary(&sp, &summary).unwrap();
    let sj = std::fs::read_to_string(&sp).unwrap();
    assert!(sj.contains("20261007"));
}
