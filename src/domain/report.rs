use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Move,
    RemoveFile,
    RemoveDir,
    CreateDir,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Move => "move",
            Action::RemoveFile => "remove-file",
            Action::RemoveDir => "remove-dir",
            Action::CreateDir => "create-dir",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveAction {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub action: Action,
    pub path: PathBuf,
    pub error: String,
}

/// 一次执行（删除空目录或拉平）实际发生了什么。失败不会中断整批任务，只记录在 `failures` 里。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub moves: Vec<MoveAction>,
    pub removed_files: Vec<PathBuf>,
    pub removed_dirs: Vec<PathBuf>,
    pub failures: Vec<Failure>,
    pub cancelled: bool,
}

impl Report {
    pub fn fail(&mut self, action: Action, path: impl Into<PathBuf>, error: impl ToString) {
        self.failures.push(Failure {
            action,
            path: path.into(),
            error: error.to_string(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
            && self.removed_files.is_empty()
            && self.removed_dirs.is_empty()
            && self.failures.is_empty()
    }
}
