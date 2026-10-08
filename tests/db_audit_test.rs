use wifiopt::db;

#[test]
fn only_select_statements_allowed() {
    for sql in db::all_statements() {
        assert!(
            sql.trim_start().to_uppercase().starts_with("SELECT"),
            "non-SELECT statement: {sql}"
        );
    }
}

#[test]
fn retries_transient_failures() {
    let mut calls = 0;
    let r = db::with_retry(|| {
        calls += 1;
        if calls < 3 {
            Err::<u8, _>(anyhow::anyhow!("boom"))
        } else {
            Ok(7)
        }
    });
    assert_eq!(r.unwrap(), 7);
    assert_eq!(calls, 3);
}
