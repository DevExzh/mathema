//! Shared controls for bounded writer output.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Optional output limits for writer requests.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct OutputLimits {
    /// Maximum UTF-8 bytes retained by a text writer. `None` is unbounded.
    pub max_bytes: Option<usize>,
    cancelled: Option<Arc<AtomicBool>>,
    // Keep the common one-control case inline. A conversion clones and
    // attaches its request control to these limits for every call, so storing
    // the first flag separately avoids allocating a Vec for ordinary
    // controlled requests while retaining support for composed controls.
    request_cancelled: Option<Arc<AtomicBool>>,
    request_cancelled_extra: Vec<Arc<AtomicBool>>,
}

impl OutputLimits {
    /// Construct an unlimited request.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_bytes: None,
            cancelled: None,
            request_cancelled: None,
            request_cancelled_extra: Vec::new(),
        }
    }

    /// Limit retained output to at most `max_bytes` bytes.
    #[must_use]
    pub const fn with_max_bytes(max_bytes: usize) -> Self {
        Self {
            max_bytes: Some(max_bytes),
            cancelled: None,
            request_cancelled: None,
            request_cancelled_extra: Vec::new(),
        }
    }

    /// Share a cancellation flag with the caller. Setting it to `true` stops
    /// the writer at its next emission boundary.
    #[must_use]
    pub fn with_cancellation(mut self, cancelled: Arc<AtomicBool>) -> Self {
        self.cancelled = Some(cancelled);
        self
    }

    /// Check a byte count and the optional cancellation flag before retaining
    /// an output buffer.
    pub(crate) fn check_bytes(&self, bytes: usize) -> crate::Result<()> {
        crate::error::request_checkpoint(1)?;
        if self.is_cancelled() {
            return Err(crate::Error::RequestCancelled);
        }
        if let Some(limit) = self.max_bytes
            && bytes > limit
        {
            return Err(crate::Error::OutputLimit { limit });
        }
        Ok(())
    }

    /// Check cancellation without charging a byte budget. Used by
    /// diagnostics-only traversals that discard temporary output.
    pub(crate) fn check_cancelled(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            Err(crate::Error::RequestCancelled)
        } else {
            Ok(())
        }
    }

    /// Clone this request with a different byte coordinate while preserving
    /// its cancellation flag. Used by writers whose textual representation
    /// expands binary data (hex output is two bytes per binary byte).
    pub(crate) fn with_byte_ceiling(&self, max_bytes: Option<usize>) -> Self {
        let mut limits = self.clone();
        limits.max_bytes = max_bytes;
        limits
    }

    /// Share cancellation with a [`RequestControl`](crate::RequestControl).
    #[must_use]
    pub fn with_request_control(mut self, control: &crate::RequestControl) -> Self {
        let handle = control.cancellation_handle();
        match self.request_cancelled.as_ref() {
            None => self.request_cancelled = Some(handle),
            Some(existing) if Arc::ptr_eq(existing, &handle) => {},
            Some(_) => self.request_cancelled_extra.push(handle),
        }
        self
    }

    #[inline]
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
            || self
                .request_cancelled
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            || self
                .request_cancelled_extra
                .iter()
                .any(|flag| flag.load(Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputFailure {
    Bytes(usize),
    Cancelled,
    Work(usize),
}

impl OutputFailure {
    pub(crate) fn into_error(self) -> crate::Error {
        match self {
            Self::Bytes(limit) => crate::Error::OutputLimit { limit },
            Self::Cancelled => crate::Error::RequestCancelled,
            Self::Work(limit) => crate::Error::LimitExceeded {
                kind: crate::error::LimitKind::Work,
                limit,
            },
        }
    }

    pub(crate) fn from_error(error: crate::Error) -> Self {
        match error {
            crate::Error::RequestCancelled => Self::Cancelled,
            crate::Error::LimitExceeded {
                kind: crate::error::LimitKind::Work,
                limit,
            } => Self::Work(limit),
            _ => unreachable!("request checkpoints only return cancellation or work errors"),
        }
    }
}
