//! `tidyfs serve`：给 Node 界面用的常驻引擎进程。
//!
//! 扫描结果保存在进程内存里（按扫描请求的 id），执行时直接使用，不需要把完整计划传给界面。

mod protocol;

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::domain::flatten::FlattenMode;
use crate::domain::options::ScanOptions;
use crate::domain::tree::is_cancelled;
use crate::engine::{self, Event, ScanOutcome, Task};
use crate::infra::config::LoadedConfig;
use crate::infra::fsutil::normalize_roots;
use crate::infra::{drives, journal, logging};

use protocol::{Envelope, Message, Request, RootApply, RootScan, Stats, TaskKind, text};

pub fn run(config: &LoadedConfig) -> Result<()> {
    let server = Arc::new(Server {
        out: Mutex::new(io::stdout()),
        options: config.scan_options(None, &[]),
        default_mode: config.flatten_mode(None),
        config_path: config.path.clone(),
        sessions: Mutex::new(HashMap::new()),
        jobs: Mutex::new(HashMap::new()),
        threads: Mutex::new(Vec::new()),
    });
    info!("serve started");

    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Envelope>(&line) {
            Ok(envelope) => {
                debug!(request = ?envelope, "request");
                Arc::clone(&server).handle(envelope);
            }
            Err(error) => {
                warn!(%error, line, "invalid request");
                server.send(
                    0,
                    true,
                    Message::Error {
                        message: format!("无法解析请求: {error}"),
                    },
                );
            }
        }
    }

    // 界面关闭了：取消所有任务，并等它们安全停下（写完操作日志）再退出。
    info!("stdin closed, stopping");
    for cancel in server.jobs.lock().expect("jobs lock").values() {
        cancel.store(true, Ordering::Relaxed);
    }
    let threads = std::mem::take(&mut *server.threads.lock().expect("threads lock"));
    for thread in threads {
        let _ = thread.join();
    }
    Ok(())
}

struct Session {
    task: Task,
    outcomes: Vec<Option<ScanOutcome>>,
}

struct Server {
    out: Mutex<io::Stdout>,
    options: ScanOptions,
    default_mode: FlattenMode,
    config_path: Option<PathBuf>,
    sessions: Mutex<HashMap<u64, Arc<Session>>>,
    jobs: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Server {
    fn send(&self, id: u64, last: bool, message: Message) {
        let Ok(mut value) = serde_json::to_value(&message) else {
            return;
        };
        value["id"] = id.into();
        value["final"] = last.into();
        let mut out = self.out.lock().expect("stdout lock");
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    }

    fn spawn(self: &Arc<Self>, work: impl FnOnce(Arc<Self>) + Send + 'static) {
        let server = Arc::clone(self);
        let handle = std::thread::spawn(move || work(server));
        let mut threads = self.threads.lock().expect("threads lock");
        threads.retain(|thread| !thread.is_finished());
        threads.push(handle);
    }

    fn fail(&self, id: u64, message: impl Into<String>) {
        self.send(
            id,
            true,
            Message::Error {
                message: message.into(),
            },
        );
    }

    fn handle(self: Arc<Self>, Envelope { id, request }: Envelope) {
        match request {
            Request::Hello => self.send(
                id,
                true,
                Message::Hello {
                    version: env!("CARGO_PKG_VERSION"),
                    flatten_mode: self.default_mode.as_str(),
                    log_dir: text(&logging::log_dir()),
                    journal_dir: text(&journal::journal_dir()),
                    config_path: self.config_path.as_deref().map(text),
                },
            ),
            Request::Drives => self.send(
                id,
                true,
                Message::Drives {
                    drives: drives::list(),
                },
            ),
            Request::Pick => {
                // 对话框无法取消，不登记到 threads，免得退出时一直等它。
                std::thread::spawn(move || {
                    let paths = rfd::FileDialog::new()
                        .set_title("选择要处理的文件夹（可多选）")
                        .pick_folders()
                        .unwrap_or_default();
                    self.send(
                        id,
                        true,
                        Message::Picked {
                            paths: paths.iter().map(|p| text(p)).collect(),
                        },
                    );
                });
            }
            Request::Scan { task, mode, roots } => {
                let task = match task {
                    TaskKind::Empty => Task::Empty,
                    TaskKind::Flatten => Task::Flatten(mode.unwrap_or(self.default_mode)),
                };
                self.spawn(move |server| server.scan(id, task, roots));
            }
            Request::Apply { scan, roots } => {
                self.spawn(move |server| server.apply(id, scan, roots));
            }
            Request::Export { scan, root } => match self.export(id, scan, root) {
                Ok(path) => self.send(id, true, Message::Exported { path: text(&path) }),
                Err(error) => self.fail(id, format!("{error:#}")),
            },
            Request::Cancel { job } => {
                if let Some(cancel) = self.jobs.lock().expect("jobs lock").get(&job) {
                    info!(job, "cancel requested");
                    cancel.store(true, Ordering::Relaxed);
                }
                self.send(id, true, Message::Ok);
            }
            Request::Forget { scan } => {
                self.sessions.lock().expect("sessions lock").remove(&scan);
                self.send(id, true, Message::Ok);
            }
        }
    }

    fn register_job(&self, id: u64) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs
            .lock()
            .expect("jobs lock")
            .insert(id, Arc::clone(&cancel));
        cancel
    }

    fn finish_job(&self, id: u64) {
        self.jobs.lock().expect("jobs lock").remove(&id);
    }

    fn scan(&self, id: u64, task: Task, roots: Vec<PathBuf>) {
        let roots = normalize_roots(roots);
        if roots.is_empty() {
            return self.fail(id, "没有选择任何目录");
        }
        let cancel = self.register_job(id);
        self.send(
            id,
            false,
            Message::ScanStarted {
                roots: roots.iter().map(|r| text(r)).collect(),
            },
        );

        let on_event = |root: usize, event: Event<'_>| {
            if let Event::Scan { stats, elapsed } = event {
                self.send(
                    id,
                    false,
                    Message::ScanProgress {
                        root,
                        stats: Stats::from(stats),
                        elapsed_ms: elapsed.as_millis() as u64,
                    },
                );
            }
        };
        let outcomes = engine::per_volume(&roots, |index| {
            let result = engine::scan_root(
                index,
                &roots[index],
                task,
                &self.options,
                &cancel,
                &on_event,
            );
            let message = match &result {
                Ok(outcome) => RootScan::from_outcome(index, outcome),
                Err(error) => RootScan::failed(
                    index,
                    &roots[index],
                    format!("{error:#}"),
                    is_cancelled(error),
                ),
            };
            self.send(id, false, Message::ScanRoot(message));
            result.ok()
        });

        self.sessions
            .lock()
            .expect("sessions lock")
            .insert(id, Arc::new(Session { task, outcomes }));
        let cancelled = cancel.load(Ordering::Relaxed);
        self.finish_job(id);
        self.send(id, true, Message::ScanDone { cancelled });
    }

    fn apply(&self, id: u64, scan: u64, roots: Vec<usize>) {
        // 扫描结果只能执行一次：执行后磁盘状态已经变了。
        let Some(session) = self.sessions.lock().expect("sessions lock").remove(&scan) else {
            return self.fail(id, "扫描结果已失效，请重新扫描");
        };
        let selected = roots
            .into_iter()
            .filter(|index| session.outcomes.get(*index).is_some_and(Option::is_some))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return self.fail(id, "没有可执行的目录");
        }
        info!(scan, task = session.task.name(), roots = ?selected, "apply requested");

        let cancel = self.register_job(id);
        self.send(
            id,
            false,
            Message::ApplyStarted {
                roots: selected.clone(),
            },
        );

        let on_event = |root: usize, event: Event<'_>| {
            if let Event::Apply { current, total } = event {
                self.send(
                    id,
                    false,
                    Message::ApplyProgress {
                        root,
                        current,
                        total,
                    },
                );
            }
        };
        let paths = selected
            .iter()
            .map(|index| {
                session.outcomes[*index]
                    .as_ref()
                    .expect("filtered above")
                    .root
                    .clone()
            })
            .collect::<Vec<_>>();
        engine::per_volume(&paths, |local| {
            let index = selected[local];
            let outcome = session.outcomes[index].as_ref().expect("filtered above");
            let applied = engine::apply_root(index, outcome, &self.options, &cancel, &on_event);
            self.send(
                id,
                false,
                Message::ApplyRoot(RootApply::new(
                    index,
                    &applied.root,
                    &applied.report,
                    applied.journal.as_deref(),
                    applied.elapsed.as_millis() as u64,
                )),
            );
        });

        let cancelled = cancel.load(Ordering::Relaxed);
        self.finish_job(id);
        self.send(id, true, Message::ApplyDone { cancelled });
    }

    fn export(&self, id: u64, scan: u64, root: usize) -> Result<PathBuf> {
        let session = self
            .sessions
            .lock()
            .expect("sessions lock")
            .get(&scan)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("扫描结果已失效，请重新扫描"))?;
        let outcome = session
            .outcomes
            .get(root)
            .and_then(Option::as_ref)
            .ok_or_else(|| anyhow::anyhow!("这个目录没有扫描结果"))?;

        let mut content = format!(
            "# tidyfs {} 扫描结果\n# {}\n# 共 {} 项\n\n",
            session.task.name(),
            outcome.root.display(),
            outcome.found.len()
        );
        for line in engine::describe(&outcome.found) {
            content.push_str(&line);
            content.push('\n');
        }
        let path = std::env::temp_dir().join(format!(
            "tidyfs-{}-{scan}-{root}-{id}.txt",
            session.task.name()
        ));
        fs::write(&path, content)?;
        Ok(path)
    }
}
