//! wai's only Agent Client Protocol client. Everything else uses these domain types.

mod actor;
mod convert;
mod subagents;
mod types;

pub use actor::{AgentHandle, program, program_name};
pub use types::*;

/// Makes `dir` (created if missing) the process cwd. Agents inherit it, so wai calls this
/// with its config dir: launching from inside a cloned repo must not let that repo's
/// `.npmrc` or `node_modules` choose what an `npx` agent runs. The SDK cannot set a child's cwd.
pub fn enter_neutral_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::env::set_current_dir(dir)
}
