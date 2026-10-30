use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wifiopt::api::{recover_interrupted, start_job, Cfg, Schedule, State};

fn temp_state(tag: &str) -> Arc<State> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("wifiopt_api_test_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("jobs")).unwrap();
    Arc::new(State {
        cfg: Cfg {
            port: 0,
            out_dir: dir,
            bin: PathBuf::from("/bin/true"),
            sched_hour: 20,
            sched_minute: 0,
            sched_stale: 50,
        },
        jobs: Mutex::new(HashMap::new()),
        schedule: Mutex::new(Schedule { hour: 20, minute: 0 }),
        last_scheduled: Mutex::new(None),
        file_cache: Mutex::new(HashMap::new()),
    })
}

fn write_running(state: &Arc<State>, id: &str) {
    let body = format!(
        r#"{{"id":"{id}","date":"20261029","regs":["reg1"],"stale":50,"upload":false,"status":"running","startedAt":"2026-10-29T20:00:00+00:00","finishedAt":null}}"#
    );
    std::fs::write(
        state.cfg.out_dir.join("jobs").join(format!("{id}.json")),
        body,
    )
    .unwrap();
}

#[test]
fn refuses_second_job_while_one_is_running() {
    let st = temp_state("guard");
    write_running(&st, "busy");
    let err = start_job(&st, "20261029", &["reg1".to_string()], 50, false).unwrap_err();
    assert!(
        err.to_string().contains("another job is running"),
        "unexpected error: {err}"
    );
}

#[test]
fn recovery_clears_stale_running_jobs() {
    let st = temp_state("recover");
    write_running(&st, "stale");
    recover_interrupted(&st);
    let job = wifiopt::api::get_job(&st, "stale").unwrap();
    assert_eq!(job.status, "failed");
    // now a new run is allowed again
    assert!(start_job(&st, "20261029", &["reg1".to_string()], 50, false).is_ok());
}

#[test]
fn rejects_bad_date_and_regs() {
    let st = temp_state("validate");
    assert!(start_job(&st, "2026-10-29", &["reg1".to_string()], 50, false).is_err());
    assert!(start_job(&st, "20261029", &["bogus".to_string()], 50, false).is_err());
    assert!(start_job(&st, "20261029", &[], 50, false).is_err());
}
