# Design: Pipeline RocksDB WAL Flush on Commit

## Problem

RocksDB WAL can grow very large (observed 2.7GB) when pipeline stages write large batches without triggering flushes. The TransactionLookup stage writes 65M+ entries in a single operation, and the WAL only flushes when:
- Memtable is full
- On shutdown
- Manual flush is called

This causes:
1. Slow `db stats` commands (~10s) due to WAL replay on DB open
2. Large disk usage from WAL files that aren't reflected in SST metrics
3. Potential data loss window (large unflushed WAL)

## Proposed Solution

Add a `flush_on_commit` configuration option to `RocksDBProvider` that the pipeline can enable. When a stage commits, the provider automatically flushes WAL/memtables.

### Key Design Decisions

1. **Pipeline configures the provider, not individual stages**
   - Stages don't need to know about WAL flushing
   - Pipeline builds a `ProviderFactory` with flush behavior enabled
   - Clean separation of concerns

2. **Use explicit flush calls, not `manual_wal_flush` option**
   - Call `db.flush()` to flush memtables to SST files
   - This allows WAL segments to be deleted
   - `flush_wal(true)` alone doesn't reduce WAL size

3. **Configurable via `RocksDBProvider` options**
   - Default: `false` (no change for non-pipeline usage)
   - Pipeline sets: `true` for WAL flush on commit

## Implementation Plan

### Step 1: Extend RocksDBProvider config

```rust
pub struct RocksDBBuilder {
    // ... existing fields
    flush_on_commit: bool,
}

impl RocksDBBuilder {
    pub fn with_flush_on_commit(mut self, enabled: bool) -> Self {
        self.flush_on_commit = enabled;
        self
    }
}
```

### Step 2: Add flush method to RocksDBProvider

```rust
impl RocksDBProvider {
    /// Flushes all memtables to SST files, allowing WAL to be truncated.
    pub fn flush(&self) -> ProviderResult<()> {
        // Flush all column families
        // This triggers memtable → SST compaction
        // After which WAL segments can be deleted
    }
}
```

### Step 3: Integrate with pipeline stage completion

The pipeline should call `rocksdb_provider.flush()` after each stage commits. This could be:

a) **In the pipeline runtime** - after stage `execute()` returns, call flush if configured
b) **In `ProviderFactory` commit hook** - if the factory has a commit/finalize method
c) **In a stage wrapper** - decorator that calls flush after the inner stage

Option (a) is cleanest - the pipeline runtime knows when stages complete.

### Step 4: Wire configuration from pipeline

```rust
// In pipeline setup
let rocksdb = RocksDBProvider::builder(path)
    .with_default_tables()
    .with_flush_on_commit(true)  // Pipeline-specific
    .build()?;
```

## API Options

### Option A: Flush on every commit (simple)
```rust
// After stage completes
if rocksdb_config.flush_on_commit {
    provider.rocksdb().flush()?;
}
```

### Option B: Flush policy (flexible)
```rust
enum FlushPolicy {
    Never,
    OnCommit,
    OnSize(usize),  // Flush when WAL exceeds N bytes
}
```

### Recommendation

Start with Option A (simple boolean). Add Option B later if needed for performance tuning.

## Files to Modify

1. `crates/storage/provider/src/providers/rocksdb/provider.rs`
   - Add `flush_on_commit` to builder
   - Add `flush()` method

2. `crates/stages/api/src/pipeline/mod.rs`
   - Call flush after stage completion if configured

3. `crates/node/builder/src/builder/mod.rs` (or similar)
   - Wire the configuration for pipeline mode

## Performance Considerations

- Flushing adds I/O overhead at stage boundaries
- For large stages, this may add seconds to commit time
- Trade-off: Slightly slower pipeline vs. faster DB opens and smaller WAL

## Testing

1. Unit test: `flush()` reduces WAL size
2. Integration test: Pipeline with flush_on_commit produces smaller WAL
3. Benchmark: Measure stage completion time with/without flush

## Future Work

- Threshold-based flushing (flush when WAL > X GB)
- Async/background flushing option
- Per-stage flush configuration
