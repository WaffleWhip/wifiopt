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
