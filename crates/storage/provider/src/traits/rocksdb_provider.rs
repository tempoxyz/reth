use crate::{
    either_writer::{RawRocksDBBatch, RocksBatchArg, RocksTxRefArg},
    providers::RocksDBProvider,
};
use reth_storage_api::StorageSettingsCache;
use reth_storage_errors::provider::ProviderResult;

/// `RocksDB` provider factory.
///
/// This trait provides access to the `RocksDB` provider
pub trait RocksDBProviderFactory: StorageSettingsCache {
    /// Returns the `RocksDB` provider.
    fn rocksdb_provider(&self) -> RocksDBProvider;

    /// Adds a pending `RocksDB` batch to be committed when this provider is committed.
    ///
    /// This allows deferring `RocksDB` commits to happen at the same time as MDBX and static file
    /// commits, ensuring atomicity across all storage backends.
    #[cfg(all(unix, feature = "rocksdb"))]
    fn set_pending_rocksdb_batch(&self, batch: rocksdb::WriteBatchWithTransaction<true>);

    /// Executes a closure with a `RocksDB` transaction for reading.
    ///
    /// This helper encapsulates all the cfg-gated `RocksDB` transaction handling for reads.
    /// On legacy MDBX-only nodes (where `any_in_rocksdb()` is false), this skips creating
    /// the RocksDB transaction entirely, avoiding unnecessary overhead.
    fn with_rocksdb_tx<F, R>(&self, f: F) -> ProviderResult<R>
    where
        F: FnOnce(RocksTxRefArg<'_>) -> ProviderResult<R>,
    {
        #[cfg(all(unix, feature = "rocksdb"))]
        {
            if self.cached_storage_settings().any_in_rocksdb() {
                let rocksdb = self.rocksdb_provider();
                let tx = rocksdb.tx();
                return f(Some(&tx));
            }
        }
        f(None)
    }

    /// Executes a closure with a `RocksDB` batch, automatically registering it for commit.
    ///
    /// This helper encapsulates all the cfg-gated `RocksDB` batch handling.
    fn with_rocksdb_batch<F, R>(&self, f: F) -> ProviderResult<R>
    where
        F: FnOnce(RocksBatchArg<'_>) -> ProviderResult<(R, Option<RawRocksDBBatch>)>,
    {
        #[cfg(all(unix, feature = "rocksdb"))]
        {
            let rocksdb = self.rocksdb_provider();
            let batch = rocksdb.batch();
            let (result, raw_batch) = f(batch)?;
            if let Some(b) = raw_batch {
                self.set_pending_rocksdb_batch(b);
            }
            Ok(result)
        }
        #[cfg(not(all(unix, feature = "rocksdb")))]
        {
            let (result, _) = f(())?;
            Ok(result)
        }
    }
}
