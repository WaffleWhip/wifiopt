use wifiopt::config::Options;
use wifiopt::db;

fn test_opts() -> Options {
    Options {
        date: "20261007".to_string(),
        regs: vec!["reg1".to_string()],
        out: "./out".to_string(),
        db_url: std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "mysql://127.0.0.1:3306/wifi_optimization".to_string()),
        stale: 50,
        upload: false,
        table: String::new(),
    }
}

#[test]
#[ignore = "needs live MySQL, run on server"]
fn fetches_discover_sample() {
    let opts = test_opts();
    let rows = db::load_discover(&opts, "reg1").expect("select discover");
    assert!(!rows.is_empty());
}
