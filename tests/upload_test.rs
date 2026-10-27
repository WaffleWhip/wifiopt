use wifiopt::output::OutputRow;
use wifiopt::upload;

fn row() -> OutputRow {
    OutputRow {
        reg: "reg2".to_string(),
        sn: "SN'\\1".to_string(),
        clusterid_2g: Some(7),
        ch_before_2g: Some(6),
        cost_before_2g: Some(0.5),
        ch_after_2g: Some(1),
        cost_after_2g: Some(0.36),
        status_2g: Some("CHANGE".to_string()),
        clusterid_5g: None,
        ch_before_5g: Some(48),
        cost_before_5g: Some(0.063),
        ch_after_5g: Some(48),
        cost_after_5g: Some(0.063),
        status_5g: Some("STAY".to_string()),
        updated_at: "2026-10-21 20:00:00".to_string(),
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
fn table_name_validated() {
    assert_eq!(upload::table_for("20261021").unwrap(), "wifi_optimize_20261021");
    assert!(upload::table_for("2026-10-21").is_err());
    assert!(upload::table_for("20261021; DROP").is_err());
    assert_eq!(
        upload::table_name("wifi_optimize_20990101").unwrap(),
        "wifi_optimize_20990101"
    );
    assert!(upload::table_name("wifi_optimize_20990101; DROP").is_err());
    assert!(upload::table_name("").is_err());
}

#[test]
fn ensure_matches_production_shape() {
    let s = upload::ensure_sql("wifi_optimize_20261021");
    assert!(s.contains("PRIMARY KEY (reg, sn)"));
    assert!(s.contains("ENGINE=MyISAM"));
    assert!(s.contains("cost_before_5g double"));
    assert!(!s.contains("node_id"), "node cols belong to the team filler, not us");
}

#[test]
fn insert_escapes_nulls_and_costs() {
    let sql = upload::insert_sql("wifi_optimize_20261021", &[row()]);
    assert!(sql.starts_with("INSERT INTO `wifi_optimize_20261021`"));
    // quote + backslash escaped
    assert!(sql.contains("'SN\\'\\\\1'"), "got: {sql}");
    // NULL cluster, 4-decimal costs like production
    assert!(sql.contains(",NULL,48,0.0630,48,0.0630,'STAY'"), "got: {sql}");
    assert!(sql.contains("0.5000"));
    assert!(sql.contains("ON DUPLICATE KEY UPDATE"));
    assert!(sql.contains("updated_at=CURRENT_TIMESTAMP"));
}

#[test]
fn insert_empty_rows_is_noop_shape() {
    let sql = upload::insert_sql("wifi_optimize_20261021", &[]);
    assert!(sql.contains("VALUES  ON DUPLICATE"));
}
