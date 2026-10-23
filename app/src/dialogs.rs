//! Native pickers run outside the frame loop; cancellation is a completed result.
use std::{path::PathBuf, sync::mpsc, thread};

#[derive(Default)]
pub struct Picker {
    result: Option<mpsc::Receiver<Result<Option<PathBuf>, &'static str>>>,
}

impl Picker {
    pub fn pending(&self) -> bool { self.result.is_some() }

    pub fn start(&mut self, task: impl FnOnce() -> Option<PathBuf> + Send + 'static) -> std::io::Result<bool> {
        if self.pending() { return Ok(false); }
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new().name("file-picker".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task))
                .map_err(|_| "The file picker could not be completed. Try again.");
            let _ = sender.send(result);
        })?;
        self.result = Some(receiver);
        Ok(true)
    }

    pub fn poll(&mut self) -> Option<Result<Option<PathBuf>, &'static str>> {
        let receiver = self.result.as_ref()?;
        match receiver.try_recv() {
            Ok(result) => { self.result = None; Some(result) }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.result = None;
                Some(Err("The file picker closed unexpectedly. Try again."))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picker_does_not_block_and_repeated_cancel_releases_it() {
        let mut picker = Picker::default();
        for selected in [None, Some(PathBuf::from("generated.pdf")), None] {
            let (release, wait) = mpsc::channel();
            let expected = selected.clone();
            assert!(picker.start(move || { wait.recv().unwrap(); selected }).unwrap());
            assert!(picker.pending());
            assert!(picker.poll().is_none());
            assert!(!picker.start(|| panic!("Duplicate picker must not run")).unwrap());
            release.send(()).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let result = loop {
                if let Some(result) = picker.poll() { break result; }
                assert!(std::time::Instant::now() < deadline);
                thread::sleep(std::time::Duration::from_millis(1));
            };
            assert_eq!(result.unwrap(), expected);
            assert!(!picker.pending());
        }
    }
}
