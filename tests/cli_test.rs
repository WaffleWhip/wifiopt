use wifiopt::config;

#[test]
fn parses_reg_subset_and_defaults_date() {
    let opts = config::parse_from(["wifiopt", "--reg", "reg1,reg2", "--out", "./out"]);
    assert_eq!(opts.regs, vec!["reg1".to_string(), "reg2".to_string()]);
}

#[test]
fn reg_all_expands_to_seven() {
    let opts = config::parse_from(["wifiopt", "--reg", "all", "--out", "./out"]);
    assert_eq!(opts.regs.len(), 7);
}
