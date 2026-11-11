//! Agent driver state: which coding-agent CLIs this machine has, which one
//! the user chose, and the model and effort remembered for each one.
//!
//! Shared between the Tauri shell and the headless CLI so both resolve the
//! saved `.tutordocs/agent.json` settings and probe drivers identically.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tutordocs_agent::catalog::{driver_catalog, DriverCatalog};
use tutordocs_agent::env::{child_env_with, AGY_METERING_ENV_VARS, QODER_METERING_ENV_VARS};
use tutordocs_agent::registry::{self, Detected, DriverStatus};
use tutordocs_agent::{
    AgentDriver, Capabilities, Effort, EventStream, JobHandle, JobSpec, ModelId,
};

/// The picker's whole state in one value: every driver, and the one in use.
#[derive(Debug, Serialize)]
pub struct AgentChoice {
    pub drivers: Vec<DriverStatus>,
    pub selected: Option<String>,
}

/// The model and effort remembered for one driver. Null is the CLI default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriverTuning {
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// Per-invocation overrides from CLI flags (`--agent`, `--model`, `--effort`).
#[derive(Debug, Clone, Default)]
pub struct AgentOverrides {
    pub driver: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// What a job-start command shows when no driver can run it.
const NO_AGENT: &str = "No agent CLI is available. Install Claude Code, Qoder or Antigravity (agy), then return to this window.";

const KNOWN_DRIVERS: [&str; 3] = ["claude", "qoder", "agy"];

pub type Detector = dyn Fn() -> Vec<Detected> + Send + Sync;

#[derive(Clone, Default)]
struct Tune {
    model: Option<ModelId>,
    effort: Option<Effort>,
}

#[derive(Clone, Default)]
struct Saved {
    driver: Option<String>,
    tuning: BTreeMap<String, Tune>,
}

struct Inner {
    settings: PathBuf,
    detect: Box<Detector>,
    cache: Mutex<Option<Arc<Vec<Detected>>>>,
    saved: Mutex<Saved>,
    overrides: AgentOverrides,
}

/// Cheap to clone: the commands hand a clone to a blocking thread.
#[derive(Clone)]
pub struct AgentsState(Arc<Inner>);

/// The file that remembers the user's choice. `tuning` is per driver id.
#[derive(Serialize, Deserialize, Default)]
struct SettingsFile {
    driver: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    tuning: BTreeMap<String, TuneFile>,
}

#[derive(Serialize, Deserialize, Default)]
struct TuneFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effort: Option<String>,
}

fn settings_path(root: &Path) -> PathBuf {
    root.join(".tutordocs").join("agent.json")
}

/// A missing or unreadable file means no choice was made, not a broken app.
/// A model or effort that cannot be passed is dropped, and the driver choice
/// beside it is kept.
fn read_saved(path: &Path) -> Saved {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Saved::default();
    };
    let Ok(file) = serde_json::from_str::<SettingsFile>(&text) else {
        return Saved::default();
    };
    let tuning = file
        .tuning
        .into_iter()
        .filter_map(|(id, raw)| {
            let tune = Tune {
                model: raw
                    .model
                    .as_deref()
                    .and_then(|model| ModelId::parse(model).ok()),
                effort: raw
                    .effort
                    .as_deref()
                    .and_then(|effort| Effort::parse(effort).ok()),
            };
            (tune.model.is_some() || tune.effort.is_some()).then_some((id, tune))
        })
        .collect();
    Saved {
        driver: file.driver,
        tuning,
    }
}

fn write_saved(path: &Path, saved: &Saved) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("could not remember the choice in {}: {err}", path.display()))?;
    }
    let file = SettingsFile {
        driver: saved.driver.clone(),
        tuning: saved
            .tuning
            .iter()
            .map(|(id, tune)| {
                (
                    id.clone(),
                    TuneFile {
                        model: tune.model.as_ref().map(|model| model.as_str().to_string()),
                        effort: tune
                            .effort
                            .as_ref()
                            .map(|effort| effort.as_str().to_string()),
                    },
                )
            })
            .collect(),
    };
    let text = serde_json::to_string(&file).map_err(|err| err.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|err| format!("could not remember the choice in {}: {err}", path.display()))
}

fn known_driver(id: &str) -> Result<(), String> {
    if KNOWN_DRIVERS.contains(&id) {
        Ok(())
    } else {
        Err(format!("{id:?} is not an agent this app knows"))
    }
}

fn put_tune(saved: &mut Saved, id: &str, edit: impl FnOnce(&mut Tune)) {
    let mut tune = saved.tuning.get(id).cloned().unwrap_or_default();
    edit(&mut tune);
    if tune.model.is_none() && tune.effort.is_none() {
        saved.tuning.remove(id);
    } else {
        saved.tuning.insert(id.to_string(), tune);
    }
}

fn first_available(detected: &[Detected]) -> Option<&Detected> {
    detected.iter().find(|d| d.status.available)
}

impl AgentsState {
    /// Detect with the CLIs installed on this machine.
    pub fn installed(root: &Path) -> Self {
        Self::new(root, Box::new(registry::detect_installed))
    }

    /// Detect with the CLIs installed on this machine and apply per-invocation
    /// CLI overrides without writing them to `.tutordocs/agent.json`.
    pub fn installed_with_overrides(root: &Path, overrides: AgentOverrides) -> Self {
        let settings = settings_path(root);
        let saved = read_saved(&settings);
        Self(Arc::new(Inner {
            settings,
            detect: Box::new(registry::detect_installed),
            cache: Mutex::new(None),
            saved: Mutex::new(saved),
            overrides,
        }))
    }

    /// Detect with `detect`, remembering the choice under the library at `root`.
    pub fn new(root: &Path, detect: Box<Detector>) -> Self {
        let settings = settings_path(root);
        let saved = read_saved(&settings);
        Self(Arc::new(Inner {
            settings,
            detect,
            cache: Mutex::new(None),
            saved: Mutex::new(saved),
            overrides: AgentOverrides::default(),
        }))
    }

    /// The cached detection, probing the first time. Blocks while it probes.
    fn detected(&self) -> Arc<Vec<Detected>> {
        let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .get_or_insert_with(|| Arc::new((self.0.detect)()))
            .clone()
    }

    /// The driver in use: the saved choice while it is still available, else
    /// the first available one in claude, qoder, agy order.
    fn selected_in(&self, detected: &[Detected]) -> Option<String> {
        let saved = self.0.saved.lock().unwrap_or_else(|e| e.into_inner());
        let kept = saved
            .driver
            .as_deref()
            .and_then(|id| detected.iter().find(|d| d.status.id == id))
            .filter(|d| d.status.available);
        kept.or_else(|| first_available(detected))
            .map(|d| d.status.id.clone())
    }

    fn view(&self, detected: &[Detected]) -> AgentChoice {
        AgentChoice {
            drivers: detected.iter().map(|d| d.status.clone()).collect(),
            selected: self.selected_in(detected),
        }
    }

    /// The driver a job should run on, probing first if nothing was probed yet.
    pub async fn driver(&self) -> Result<Arc<dyn AgentDriver>, String> {
        let state = self.clone();
        blocking(move || state.driver_now()).await
    }

    pub fn driver_now(&self) -> Result<Arc<dyn AgentDriver>, String> {
        let detected = self.detected();
        let id = if let Some(ref forced) = self.0.overrides.driver {
            known_driver(forced)?;
            let Some(driver) = detected.iter().find(|d| d.status.id == *forced) else {
                return Err(format!("{forced:?} is not an agent this app knows"));
            };
            if !driver.status.available {
                let why = driver
                    .status
                    .reason
                    .as_deref()
                    .unwrap_or("it was not found");
                return Err(format!("{forced} is not available: {why}"));
            }
            forced.clone()
        } else {
            self.selected_in(&detected)
                .ok_or_else(|| NO_AGENT.to_string())?
        };
        let inner = detected
            .iter()
            .find(|driver| driver.status.id == id)
            .and_then(|driver| driver.driver.clone())
            .ok_or_else(|| NO_AGENT.to_string())?;
        let mut tune = self
            .0
            .saved
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .tuning
            .get(&id)
            .cloned()
            .unwrap_or_default();
        if let Some(ref raw_model) = self.0.overrides.model {
            tune.model = Some(ModelId::parse(raw_model)?);
        }
        if let Some(ref raw_effort) = self.0.overrides.effort {
            tune.effort = Some(Effort::parse(raw_effort)?);
        }
        Ok(Arc::new(TunedDriver {
            inner,
            model: tune.model,
            effort: tune.effort,
        }))
    }

    pub fn list(&self) -> AgentChoice {
        self.view(&self.detected())
    }

    /// Probe again, for a CLI installed or updated since the last probe.
    pub fn refresh(&self) -> AgentChoice {
        let mut cache = self.0.cache.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = Arc::new((self.0.detect)());
        *cache = Some(fresh.clone());
        drop(cache);
        self.view(&fresh)
    }

    /// Use `id` from now on and across restarts. Only a driver that is
    /// available can be chosen.
    pub fn select(&self, id: &str) -> Result<AgentChoice, String> {
        let detected = self.detected();
        let Some(driver) = detected.iter().find(|d| d.status.id == id) else {
            return Err(format!("{id:?} is not an agent this app knows"));
        };
        if !driver.status.available {
            let why = driver
                .status
                .reason
                .as_deref()
                .unwrap_or("it was not found");
            return Err(format!("{id} is not available: {why}"));
        }
        self.update_saved(|saved| saved.driver = Some(id.to_string()))?;
        Ok(self.view(&detected))
    }

    fn update_saved(&self, change: impl FnOnce(&mut Saved)) -> Result<(), String> {
        let mut saved = self.0.saved.lock().unwrap_or_else(|err| err.into_inner());
        let mut next = saved.clone();
        change(&mut next);
        write_saved(&self.0.settings, &next)?;
        *saved = next;
        Ok(())
    }

    fn tuning_view(&self, id: &str) -> DriverTuning {
        let saved = self.0.saved.lock().unwrap_or_else(|err| err.into_inner());
        let tune = saved.tuning.get(id);
        DriverTuning {
            model: tune
                .and_then(|tune| tune.model.as_ref().map(|model| model.as_str().to_string())),
            effort: tune.and_then(|tune| {
                tune.effort
                    .as_ref()
                    .map(|effort| effort.as_str().to_string())
            }),
        }
    }

    pub fn tuning_of(&self, id: &str) -> Result<DriverTuning, String> {
        known_driver(id)?;
        Ok(self.tuning_view(id))
    }

    pub fn set_model(&self, id: &str, model: Option<String>) -> Result<DriverTuning, String> {
        known_driver(id)?;
        let parsed = match model {
            Some(raw) => Some(ModelId::parse(&raw)?),
            None => None,
        };
        let drop_effort = self.saved_effort_not_offered(id, parsed.as_ref());
        self.update_saved(|saved| {
            put_tune(saved, id, |tune| {
                tune.model = parsed;
                if drop_effort {
                    tune.effort = None;
                }
            })
        })?;
        Ok(self.tuning_view(id))
    }

    /// True when a probed catalog lists this model and omits the saved effort.
    /// A probe that did not run leaves the saved effort alone.
    fn saved_effort_not_offered(&self, id: &str, model: Option<&ModelId>) -> bool {
        let Some(effort) = self.tuning_view(id).effort else {
            return false;
        };
        let Some(catalog) = self.probed_catalog(id) else {
            return false;
        };
        let wanted = model.map(ModelId::as_str);
        if let Some(option) = catalog
            .models
            .iter()
            .find(|option| option.id.as_deref() == wanted)
        {
            return !option.efforts.iter().any(|level| level.as_str() == effort);
        }
        if catalog.accepts_custom_model {
            return !catalog.efforts.iter().any(|level| level.as_str() == effort);
        }
        false
    }

    pub fn set_effort(&self, id: &str, effort: Option<String>) -> Result<DriverTuning, String> {
        known_driver(id)?;
        let parsed = match effort {
            Some(raw) => Some(Effort::parse(&raw)?),
            None => None,
        };
        self.update_saved(|saved| put_tune(saved, id, |tune| tune.effort = parsed))?;
        Ok(self.tuning_view(id))
    }

    pub fn list_models(&self, id: &str) -> Result<DriverCatalog, String> {
        known_driver(id)?;
        Ok(self
            .probed_catalog(id)
            .unwrap_or_else(|| driver_catalog(id, "", None)))
    }

    /// Help text, and agy's model listing, from the installed binary. `None`
    /// when the binary is missing or `--help` failed, so a caller can tell a
    /// failed probe from a help text that simply lists nothing.
    fn probed_catalog(&self, id: &str) -> Option<DriverCatalog> {
        let binary = self
            .detected()
            .iter()
            .find(|driver| driver.status.id == id)
            .and_then(|driver| driver.status.binary.clone())?;
        let help = run_cli(&binary, &["--help"], scrub_for(id)).ok()?;
        let models = if id == "agy" {
            run_cli(&binary, &["models"], scrub_for(id)).ok()
        } else {
            None
        };
        Some(driver_catalog(id, &help, models.as_deref()))
    }
}

/// The picked model and effort, applied as the job reaches the real driver.
struct TunedDriver {
    inner: Arc<dyn AgentDriver>,
    model: Option<ModelId>,
    effort: Option<Effort>,
}

#[tutordocs_agent::async_trait]
impl AgentDriver for TunedDriver {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    async fn start(&self, mut job: JobSpec) -> tutordocs_agent::anyhow::Result<JobHandle> {
        job.model = self.model.clone();
        job.effort = self.effort.clone();
        self.inner.start(job).await
    }

    async fn resume(
        &self,
        session: &str,
        mut job: JobSpec,
    ) -> tutordocs_agent::anyhow::Result<JobHandle> {
        job.model = self.model.clone();
        job.effort = self.effort.clone();
        self.inner.resume(session, job).await
    }

    fn events(&self, handle: &JobHandle) -> EventStream {
        self.inner.events(handle)
    }

    async fn cancel(&self, handle: &JobHandle) -> tutordocs_agent::anyhow::Result<()> {
        self.inner.cancel(handle).await
    }
}

fn scrub_for(id: &str) -> &'static [&'static str] {
    match id {
        "agy" => AGY_METERING_ENV_VARS,
        "qoder" => QODER_METERING_ENV_VARS,
        _ => &[],
    }
}

/// Run one listing (`--help` or `agy models`) and return both streams.
/// Metering variables are removed before the spawn. A failure or a hang of
/// more than ten seconds is an error; the caller turns that into a catalog
/// that offers only the CLI default.
fn run_cli(binary: &Path, args: &[&str], extra: &[&str]) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new(binary)
        .args(args)
        .env_clear()
        .envs(child_env_with(extra))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "the CLI listing did not start".to_string())?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stdout.take() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stderr.take() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > Duration::from_secs(10) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_thread.join();
                let _ = err_thread.join();
                return Err("the CLI listing timed out".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = out_thread.join();
                let _ = err_thread.join();
                return Err("the CLI listing could not be checked".to_string());
            }
        }
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    if !status.success() {
        return Err("the CLI listing failed".to_string());
    }
    let mut text = stdout;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&stderr);
    Ok(text)
}

/// Run `work` off the async executor when a Tokio runtime is present.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|e| format!("the agent check did not finish: {e}"))?
    } else {
        work()
    }
}
