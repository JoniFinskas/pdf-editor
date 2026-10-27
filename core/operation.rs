//! Cooperative cancellation at safe transaction boundaries.
use crate::{Engine, Result, err};
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationProgress {
    pub label: String,
    pub completed: usize,
    pub total: usize,
}

#[derive(Clone, Default)]
pub struct OperationControl(Arc<Control>);
#[derive(Default)]
struct Control {
    cancelled: AtomicBool,
    progress: Mutex<Option<OperationProgress>>,
}
impl OperationControl {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    pub fn progress(&self) -> Option<OperationProgress> {
        self.0
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
thread_local! {
    static CURRENT: RefCell<Option<OperationControl>> = const { RefCell::new(None) };
}

/// Runs a job with its own control; nested scopes restore the previous control.
pub fn with_control<T>(control: &OperationControl, f: impl FnOnce() -> T) -> T {
    struct Reset(Option<OperationControl>);
    impl Drop for Reset {
        fn drop(&mut self) {
            CURRENT.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _reset = Reset(CURRENT.with(|slot| slot.replace(Some(control.clone()))));
    f()
}

pub(crate) fn check_cancelled() -> Result<()> {
    CURRENT.with(|slot| {
        if slot.borrow().as_ref().is_some_and(OperationControl::is_cancelled) {
            Err(err("Operation cancelled"))
        } else { Ok(()) }
    })
}

/// Checks cancellation before the next safe unit of work or transaction commit.
pub fn checkpoint(label: &str, completed: usize, total: usize) -> Result<()> {
    CURRENT.with(|slot| {
        if let Some(control) = slot.borrow().as_ref() {
            if control.is_cancelled() {
                return Err(err("Operation cancelled"));
            }
            *control.0.progress.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(OperationProgress {
                    label: label.chars().take(120).collect(),
                    completed: completed.min(total),
                    total,
                });
        }
        Ok(())
    })
}
impl Engine {
    pub fn with_operation<T>(
        &self,
        control: &OperationControl,
        f: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        with_control(control, || {
            checkpoint("Starting", 0, 1)?;
            f()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_scoped_and_does_not_reach_the_next_job() {
        let first = OperationControl::default();
        with_control(&first, || {
            checkpoint("Pages", 1, 3).unwrap();
            assert_eq!(first.progress().unwrap().completed, 1);
            first.cancel();
            assert!(checkpoint("Pages", 2, 3).is_err());
            let next = OperationControl::default();
            with_control(&next, || checkpoint("Next", 0, 1)).unwrap();
            assert!(!next.is_cancelled());
            assert!(checkpoint("Pages", 3, 3).is_err());
        });
        checkpoint("Uncontrolled", 0, 1).unwrap();
    }
}
