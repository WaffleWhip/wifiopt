//! server: control API entry point. All logic lives in [`wifiopt::api`].

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use wifiopt::api::{
    load_cfg, load_schedule, read_req, respond, route, spawn_scheduler, State,
};
fn main() {
    let cfg = load_cfg();
    let sched = load_schedule(&cfg);
    println!(
        "server on :{} out={} daily={:02}:{:02}",
        cfg.port,
        cfg.out_dir.display(),
        sched.hour,
        sched.minute
    );
    let state = Arc::new(State {
        cfg,
        jobs: Mutex::new(HashMap::new()),
        schedule: Mutex::new(sched),
        last_scheduled: Mutex::new(None),
        file_cache: Mutex::new(HashMap::new()),
    });
    spawn_scheduler(Arc::clone(&state));
    wifiopt::api::recover_interrupted(&state);
    let listener =
        TcpListener::bind(format!("0.0.0.0:{}", state.cfg.port)).expect("bind api port");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let st = Arc::clone(&state);
        std::thread::spawn(move || {
            let (status, body) = match read_req(&stream) {
                Ok(req) => route(&st, &req),
                Err(_) => (400, serde_json::json!({"error": "bad request"}).to_string()),
            };
            let text = match status {
                200 => "OK",
                202 => "Accepted",
                400 => "Bad Request",
                404 => "Not Found",
                _ => "Error",
            };
            respond(&mut stream, status, text, &body);
        });
    }
}
