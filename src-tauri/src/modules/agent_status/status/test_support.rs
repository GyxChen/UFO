//! 测试辅助：临时目录。
//!
//! 只在测试构建下编译，供 `hook` / `tokens` 等子模块共享，避免各自复制一份。

use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

/// 目录名自增序号，保证同一进程内每次 `new` 都拿到不同目录。
///
/// 仅靠 tag 不够：cargo test 在同一进程内并行跑用例，
/// 同 tag 的用例会互相 `remove_dir_all`，导致对方刚写入的文件凭空消失。
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// 测试用临时目录，Drop 时自动清理；名字含进程号、标签与自增序号，避免并行测试互相干扰。
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = env::temp_dir().join(format!("ufo-test-{}-{tag}-{id}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// 在临时目录下写入一个文件（自动建父目录），返回其完整路径。
    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("file parent")).expect("create file dir");
        fs::write(&path, contents).expect("write temp file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
