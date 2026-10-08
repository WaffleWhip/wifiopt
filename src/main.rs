use wifiopt::config;

fn main() {
    let opts = config::parse_args();
    println!("wifiopt date={} regs={:?} out={}", opts.date, opts.regs, opts.out);
}
