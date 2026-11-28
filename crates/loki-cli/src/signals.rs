//! Ctrl+C that degrades safely.
//!
//! `tokio::signal::ctrl_c()` returns `Err` immediately if the handler can't be installed (for
//! example a Windows child process without a console). Treating that error as a key press would
//! cancel every reply instantly and spin the prompt loop. Here an installation error means
//! "Ctrl+C never fires"; `/stop` and `/quit` keep working.

/// Resolves when the user presses Ctrl+C; never resolves if the handler can't be installed.
pub async fn ctrl_c() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}
