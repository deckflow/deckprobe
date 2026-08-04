use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

use web_time::Instant;

use crate::{Budget, CostSnapshot, DeckProbeError, Result};

/// A seekable reader returned by a [`ProbeSource`].
///
/// Format crates depend on this capability instead of concrete files so the
/// same parsers can run against local files, stdin buffers, and browser bytes.
pub trait ProbeReader: Read + Seek + Send {}

impl<T> ProbeReader for T where T: Read + Seek + Send {}

pub type BoxedProbeReader = Box<dyn ProbeReader>;

/// Re-openable input for a probe execution.
///
/// Drivers may open the source more than once for independent bounded paths.
/// Implementations must therefore return a new reader positioned at byte zero
/// on every call to `open`.
pub trait ProbeSource: Send + Sync {
    fn display_name(&self) -> &str;
    fn source_kind(&self) -> &str;
    fn len(&self) -> u64;
    fn open(&self) -> Result<BoxedProbeReader>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn extension(&self) -> Option<String> {
        Path::new(self.display_name())
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
    }
}

/// In-memory source used by WASM, stdin, tests, and embedders.
#[derive(Clone)]
pub struct MemorySource {
    display_name: String,
    source_kind: String,
    bytes: Arc<[u8]>,
}

impl MemorySource {
    pub fn new(display_name: impl Into<String>, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::with_kind(display_name, "memory", bytes)
    }

    pub fn with_kind(
        display_name: impl Into<String>,
        source_kind: impl Into<String>,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            source_kind: source_kind.into(),
            bytes: bytes.into(),
        }
    }

    pub fn bytes(&self) -> Arc<[u8]> {
        self.bytes.clone()
    }
}

impl ProbeSource for MemorySource {
    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn source_kind(&self) -> &str {
        &self.source_kind
    }

    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn open(&self) -> Result<BoxedProbeReader> {
        Ok(Box::new(Cursor::new(self.bytes.clone())))
    }
}

#[derive(Debug)]
struct CostState {
    physical_bytes_read: u64,
    expanded_bytes: u64,
    random_reads: u64,
    started_at: Instant,
    budget_failure: Option<String>,
}

pub struct ProbeContext {
    source: Arc<dyn ProbeSource>,
    budget: Budget,
    state: Arc<Mutex<CostState>>,
}

impl ProbeContext {
    pub fn new(source: Arc<dyn ProbeSource>, budget: Budget) -> Result<Self> {
        if source.len() > usize::MAX as u64 {
            return Err(DeckProbeError::InvalidRequest(format!(
                "source {} is too large for this platform",
                source.display_name()
            )));
        }
        Ok(Self {
            source,
            budget,
            state: Arc::new(Mutex::new(CostState {
                physical_bytes_read: 0,
                expanded_bytes: 0,
                random_reads: 0,
                started_at: Instant::now(),
                budget_failure: None,
            })),
        })
    }

    pub fn from_source<S>(source: S, budget: Budget) -> Result<Self>
    where
        S: ProbeSource + 'static,
    {
        Self::new(Arc::new(source), budget)
    }

    pub fn display_name(&self) -> &str {
        self.source.display_name()
    }

    pub fn source_kind(&self) -> &str {
        self.source.source_kind()
    }

    pub fn file_size(&self) -> u64 {
        self.source.len()
    }

    pub fn extension(&self) -> Option<String> {
        self.source.extension()
    }

    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    pub fn read_prefix(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut reader = self.open_budgeted_reader()?;
        let mut bytes = vec![0; len.min(self.file_size() as usize)];
        reader.read_exact(&mut bytes).map_err(map_budget_io_error)?;
        Ok(bytes)
    }

    pub fn read_all(&mut self) -> Result<Vec<u8>> {
        if self.file_size() > self.budget.max_physical_bytes {
            return Err(DeckProbeError::BudgetExceeded(format!(
                "file size {} exceeds physical read budget {}",
                self.file_size(),
                self.budget.max_physical_bytes
            )));
        }
        let mut reader = self.open_budgeted_reader()?;
        let mut bytes = Vec::with_capacity(self.file_size() as usize);
        reader
            .read_to_end(&mut bytes)
            .map_err(map_budget_io_error)?;
        Ok(bytes)
    }

    pub fn open_budgeted_reader(&self) -> Result<BudgetedReader<BoxedProbeReader>> {
        Ok(BudgetedReader {
            inner: self.source.open()?,
            budget: self.budget.clone(),
            state: self.state.clone(),
            position: 0,
        })
    }

    pub fn record_expanded(&self, amount: u64) -> Result<()> {
        let mut state = self.state.lock().expect("cost state poisoned");
        let next = state.expanded_bytes.saturating_add(amount);
        if next > self.budget.max_expanded_bytes {
            let message = format!(
                "expanded bytes {next} exceed budget {}",
                self.budget.max_expanded_bytes
            );
            state.budget_failure = Some(message.clone());
            return Err(DeckProbeError::BudgetExceeded(message));
        }
        state.expanded_bytes = next;
        Ok(())
    }

    pub fn check_time(&self) -> Result<()> {
        let mut state = self.state.lock().expect("cost state poisoned");
        if state.started_at.elapsed() > self.budget.timeout {
            let message = format!("timeout of {} ms exceeded", self.budget.timeout.as_millis());
            state.budget_failure = Some(message.clone());
            return Err(DeckProbeError::BudgetExceeded(message));
        }
        Ok(())
    }

    /// Returns the first budget failure observed by a budgeted reader.
    ///
    /// Container parsers sometimes erase the nested I/O error text (for
    /// example, ZIP may report only `i/o error`). Keeping the failure on the
    /// shared context preserves the stable `BUDGET_EXCEEDED` classification.
    pub fn budget_failure(&self) -> Option<String> {
        self.state
            .lock()
            .expect("cost state poisoned")
            .budget_failure
            .clone()
    }

    pub fn cost_snapshot(&self, include_elapsed: bool) -> CostSnapshot {
        let state = self.state.lock().expect("cost state poisoned");
        CostSnapshot {
            physical_bytes_read: state.physical_bytes_read,
            expanded_bytes: state.expanded_bytes,
            random_reads: state.random_reads,
            elapsed_ms: include_elapsed.then(|| state.started_at.elapsed().as_secs_f64() * 1000.0),
        }
    }
}

fn map_budget_io_error(error: std::io::Error) -> DeckProbeError {
    if error.to_string().contains("deckprobe") {
        DeckProbeError::BudgetExceeded(error.to_string())
    } else {
        DeckProbeError::Io(error)
    }
}

pub struct BudgetedReader<R> {
    inner: R,
    budget: Budget,
    state: Arc<Mutex<CostState>>,
    position: u64,
}

impl<R: Read> Read for BudgetedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        {
            let mut state = self.state.lock().expect("cost state poisoned");
            if state.started_at.elapsed() > self.budget.timeout {
                let message = "deckprobe timeout budget exceeded".to_owned();
                state.budget_failure = Some(message.clone());
                return Err(std::io::Error::other(message));
            }
            if state.physical_bytes_read >= self.budget.max_physical_bytes {
                let message = "deckprobe physical read budget exceeded".to_owned();
                state.budget_failure = Some(message.clone());
                return Err(std::io::Error::other(message));
            }
        }
        let remaining = {
            let state = self.state.lock().expect("cost state poisoned");
            (self.budget.max_physical_bytes - state.physical_bytes_read) as usize
        };
        let allowed = buffer.len().min(remaining);
        let read = self.inner.read(&mut buffer[..allowed])?;
        let mut state = self.state.lock().expect("cost state poisoned");
        state.physical_bytes_read += read as u64;
        self.position += read as u64;
        Ok(read)
    }
}

impl<R: Seek> Seek for BudgetedReader<R> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let previous = self.position;
        let next = self.inner.seek(position)?;
        self.position = next;
        if next != previous {
            let mut state = self.state.lock().expect("cost state poisoned");
            state.random_reads += 1;
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};
    use std::sync::Arc;

    use super::{MemorySource, ProbeContext};
    use crate::{Budget, ProbeLevel};

    #[test]
    fn memory_source_is_reopenable_and_budgeted() {
        let source = MemorySource::new("sample.pdf", Arc::<[u8]>::from(&b"%PDF-test"[..]));
        let context = ProbeContext::from_source(source, Budget::for_level(ProbeLevel::Header))
            .expect("context");

        let mut first = context.open_budgeted_reader().expect("first reader");
        let mut prefix = [0; 5];
        first.read_exact(&mut prefix).expect("prefix");
        assert_eq!(&prefix, b"%PDF-");

        let mut second = context.open_budgeted_reader().expect("second reader");
        second.seek(SeekFrom::End(-4)).expect("seek");
        let mut suffix = String::new();
        second.read_to_string(&mut suffix).expect("suffix");
        assert_eq!(suffix, "test");
        assert_eq!(context.extension().as_deref(), Some("pdf"));
        assert_eq!(context.cost_snapshot(false).physical_bytes_read, 9);
    }

    #[test]
    fn budget_failure_survives_container_error_wrapping() {
        let source = MemorySource::new("sample.zip", Arc::<[u8]>::from(&b"0123456789"[..]));
        let mut budget = Budget::for_level(ProbeLevel::Header);
        budget.max_physical_bytes = 5;
        let context = ProbeContext::from_source(source, budget).expect("context");
        let mut reader = context.open_budgeted_reader().expect("reader");
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).expect_err("budget error");

        assert_eq!(bytes, b"01234");
        assert_eq!(
            context.budget_failure().as_deref(),
            Some("deckprobe physical read budget exceeded")
        );
    }
}
