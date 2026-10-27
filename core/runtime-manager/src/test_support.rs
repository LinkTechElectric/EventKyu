use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::{HttpResponse, RangeRequest, RuntimeError, Transport, VersionProbe};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

pub(crate) struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub(crate) fn new(name: &str) -> Self {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "eventkyu-runtime-manager-{name}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test directory");
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedGet {
    pub(crate) url: String,
    pub(crate) range: Option<RangeRequest>,
}

enum TransportStep {
    Response(HttpResponse),
    Error(String),
}

#[derive(Default)]
pub(crate) struct MockTransport {
    steps: Mutex<VecDeque<TransportStep>>,
    gets: Mutex<Vec<RecordedGet>>,
    heads: Mutex<Vec<String>>,
}

impl MockTransport {
    pub(crate) fn respond(&self, response: HttpResponse) {
        self.steps
            .lock()
            .expect("lock steps")
            .push_back(TransportStep::Response(response));
    }

    pub(crate) fn fail(&self, message: &str) {
        self.steps
            .lock()
            .expect("lock steps")
            .push_back(TransportStep::Error(message.to_owned()));
    }

    pub(crate) fn gets(&self) -> Vec<RecordedGet> {
        self.gets.lock().expect("lock gets").clone()
    }
}

impl Transport for MockTransport {
    fn get(&self, url: &str, range: Option<RangeRequest>) -> Result<HttpResponse, RuntimeError> {
        self.gets.lock().expect("lock gets").push(RecordedGet {
            url: url.to_owned(),
            range,
        });
        match self
            .steps
            .lock()
            .expect("lock steps")
            .pop_front()
            .expect("scripted GET response")
        {
            TransportStep::Response(response) => Ok(response),
            TransportStep::Error(message) => Err(RuntimeError::Transport {
                url: url.to_owned(),
                message,
            }),
        }
    }

    fn head(&self, url: &str) -> Result<HttpResponse, RuntimeError> {
        self.heads.lock().expect("lock heads").push(url.to_owned());
        Ok(HttpResponse::new(200, Vec::new()))
    }
}

#[derive(Default)]
pub(crate) struct MockVersionProbe {
    results: Mutex<VecDeque<Result<Option<String>, String>>>,
}

impl MockVersionProbe {
    pub(crate) fn returns(&self, version: Option<&str>) {
        self.results
            .lock()
            .expect("lock probe results")
            .push_back(Ok(version.map(str::to_owned)));
    }

    pub(crate) fn fails(&self, message: &str) {
        self.results
            .lock()
            .expect("lock probe results")
            .push_back(Err(message.to_owned()));
    }
}

impl VersionProbe for MockVersionProbe {
    fn probe(&self, executable: &Path, _args: &[String]) -> Result<Option<String>, RuntimeError> {
        match self
            .results
            .lock()
            .expect("lock probe results")
            .pop_front()
            .unwrap_or(Ok(None))
        {
            Ok(version) => Ok(version),
            Err(message) => Err(RuntimeError::VersionProbe {
                path: executable.to_path_buf(),
                message,
            }),
        }
    }
}
