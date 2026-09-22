//! Opt-in, bounded diagnostics. Buffer in memory; write only after capture ends.
use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(serde::Serialize)]
struct Sample {
    kind: &'static str,
    light: Option<usize>,
    at_ms: f64,
    duration_ms: f64,
    ok: bool,
}

struct Capture {
    started: Instant,
    deadline: Instant,
    samples: Vec<Sample>,
}

static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);
const MAX_SAMPLES: usize = 100_000;

pub fn record(kind: &'static str, started: Instant, light: Option<usize>, ok: bool) {
    let finished = Instant::now();
    let mut capture = CAPTURE.lock().unwrap();
    if let Some(capture) = capture.as_mut() {
        if started >= capture.started
            && finished <= capture.deadline
            && capture.samples.len() < MAX_SAMPLES
        {
            capture.samples.push(Sample {
                kind,
                light,
                at_ms: (started - capture.started).as_secs_f64() * 1000.0,
                duration_ms: (finished - started).as_secs_f64() * 1000.0,
                ok,
            });
        }
    }
}

struct Session(Instant);
impl Drop for Session {
    fn drop(&mut self) {
        let mut capture = CAPTURE.lock().unwrap();
        if capture
            .as_ref()
            .is_some_and(|capture| capture.started == self.0)
        {
            capture.take();
        }
    }
}

pub async fn capture(directory: &Path, seconds: u32) -> Result<String, String> {
    if !(5..=300).contains(&seconds) {
        return Err("trace duration must be between 5 and 300 seconds".into());
    }
    let started = Instant::now();
    {
        let mut capture = CAPTURE.lock().unwrap();
        if capture.is_some() {
            return Err("a breathing timing capture is already running".into());
        }
        *capture = Some(Capture {
            started,
            deadline: started + Duration::from_secs(u64::from(seconds)),
            samples: Vec::new(),
        });
    }
    let session = Session(started);
    tokio::time::sleep(Duration::from_secs(u64::from(seconds))).await;
    let capture = CAPTURE
        .lock()
        .unwrap()
        .take()
        .expect("session owns the capture");
    drop(session);
    let report = serde_json::json!({
        "duration_seconds": seconds,
        "sample_limit_reached": capture.samples.len() == MAX_SAMPLES,
        "samples": capture.samples,
    });
    let directory = directory.join("diagnostics");
    let path = directory.join(format!(
        "breathing-timing-{}.json",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    ));
    let contents = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    std::fs::write(&path, contents).map_err(|error| error.to_string())?;
    Ok(path.display().to_string())
}
