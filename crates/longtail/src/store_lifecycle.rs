//! Closing a block store on every exit path, not just the successful one.

use std::sync::Arc;

use longtail_store::block_store::BlockStore;

use crate::error::LongtailError;

/// Flush and close `store`, then return `outcome`.
///
/// Both of the store's end-of-run obligations live behind `close()`: warm-cache
/// write-backs, and the LRU sweep that trims the on-disk cache to its byte
/// budget. Hanging those off the success path leaves a cancelled or failed run
/// with an oversized cache and unwritten blocks until some later run happens to
/// succeed — and cancellation is a normal flow here, not an exceptional one.
///
/// A cleanup failure never replaces the operation's own error. The caller needs
/// to know why the operation failed, not why the tidy-up afterwards did; the
/// cleanup error is logged and dropped in that case.
///
/// Cheap when there is nothing to do: `flush` is a no-op on a `ReadOnly` store
/// (it still drains parked prefetch tasks, which is what a cancel wants), and
/// the eviction sweep only runs when the caller set a cache byte budget.
pub(crate) async fn finish_store<T>(
    store: &Arc<dyn BlockStore>,
    outcome: Result<T, LongtailError>,
) -> Result<T, LongtailError> {
    let cleanup = async {
        store.flush().await?;
        store.close().await
    }
    .await;

    match (outcome, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(cleanup)) => Err(cleanup.into()),
        (Err(op), Ok(())) => Err(op),
        (Err(op), Err(cleanup)) => {
            tracing::warn!(
                cleanup_error = %cleanup,
                "store flush/close failed while unwinding; reporting the original error"
            );
            Err(op)
        }
    }
}
