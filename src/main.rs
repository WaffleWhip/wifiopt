use wifiopt::config;
use wifiopt::pipeline;

fn main() {
    let opts = config::parse_args();
    match pipeline::run(&opts) {
        Ok(s) => println!(
            "[{}] done date={} rows={}",
            pipeline::ts(),
            s.date,
            s.rows
        ),
        Err(e) => {
            eprintln!("[{}] wifiopt failed: {e:#}", pipeline::ts());
            std::process::exit(1);
        }
    }
}
