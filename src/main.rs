use wifiopt::config;
use wifiopt::pipeline;

fn main() {
    let opts = config::parse_args();
    match pipeline::run(&opts) {
        Ok(s) => println!("done date={} rows={}", s.date, s.rows),
        Err(e) => {
            eprintln!("wifiopt failed: {e:#}");
            std::process::exit(1);
        }
    }
}
